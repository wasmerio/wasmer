//! The `cpu` and `io` controllers' state of a cgroup (`kernel/sched/core.c`'s `cpu_files`,
//! `kernel/sched/fair.c`'s CFS bandwidth control, `block/blk-cgroup.c`'s `io.stat`,
//! `block/blk-throttle.c`'s `io.max`), and pressure stall information (`kernel/sched/psi.c`).
//!
//! `cpu.weight` is kept and shown: the host schedules the threads, the kernel has no run queue
//! to weigh. `cpu.max` is enforced: the CPU time a task is charged with at each tick
//! (`account_cfs_rq_runtime`) comes out of the runtime of its cgroup and of every ancestor with
//! a quota, and a task that finds one run out is throttled until that cgroup's next period
//! refills it (`throttle_cfs_rq`, `do_sched_cfs_period_timer`). A period turns when the cgroup
//! is next charged or looked at, so an idle cgroup counts no periods, as Linux's period timer
//! stops when idle. The pressure of a cgroup's CPU is the time it was throttled: its tasks were
//! runnable and could not run, all of them at once.
//!
//! `io.stat` counts the bytes and calls of the reads and writes of files through the VFS, by the
//! device of their filesystem: there is no block layer below. `io.max` is kept, not enforced.

use std::{collections::BTreeMap, time::Duration};

use wasixv2_syscalls::Errno;

/// `CGROUP_WEIGHT_MIN`, `CGROUP_WEIGHT_DFL`, `CGROUP_WEIGHT_MAX`.
const WEIGHT_MIN: u64 = 1;
const WEIGHT_DFL: u64 = 100;
const WEIGHT_MAX: u64 = 10_000;
/// `NICE_0_LOAD` without its fixed point.
const NICE_0_WEIGHT: u64 = 1024;
/// `sched_prio_to_weight`: the weight of each nice value from -20 to 19.
const PRIO_TO_WEIGHT: [u64; 40] = [
    88761, 71755, 56483, 46273, 36291, 29154, 23254, 18705, 14949, 11916, 9548, 7620, 6100, 4904,
    3906, 3121, 2501, 1991, 1586, 1277, 1024, 820, 655, 526, 423, 335, 272, 215, 172, 137, 110, 87,
    70, 56, 45, 36, 29, 23, 18, 15,
];
/// `default_bw_period_us`, `min_bw_quota_period_us`, `max_bw_quota_period_us`.
const PERIOD_DFL_US: u64 = 100_000;
const MIN_US: u64 = 1_000;
const MAX_PERIOD_US: u64 = 1_000_000;
/// `max_bw_runtime_us`: `MAX_BW`, what a quota shifted by `BW_SHIFT` still holds.
const MAX_RUNTIME_US: u64 = (1 << (64 - 20)) - 1;

fn us(us: u64) -> Duration {
    Duration::from_micros(us)
}

/// `DIV_ROUND_CLOSEST_ULL`.
fn div_round(n: u64, d: u64) -> u64 {
    (n + d / 2) / d
}

/// The `cpu` controller's state of a cgroup (`struct task_group` with its `cfs_bandwidth`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bandwidth {
    /// `tg->shares`, unscaled.
    shares: u64,
    idle: bool,
    /// `cpu.max`: the runtime of a period, none for `max`.
    quota: Option<Duration>,
    period: Duration,
    /// `cpu.max.burst`: unused runtime a period may carry over.
    burst: Duration,
    /// Where the current period started; none until the cgroup ran with a quota.
    period_start: Option<Duration>,
    /// Runtime left in the current period, in nanoseconds; negative once overrun.
    runtime: i128,
    /// Runtime used in the current period.
    used: Duration,
    nr_periods: u64,
    nr_throttled: u64,
    nr_bursts: u64,
    throttled_time: Duration,
    burst_time: Duration,
    /// Throttled since then, until the period ends.
    throttled_since: Option<Duration>,
    psi: Psi,
}

impl Default for Bandwidth {
    fn default() -> Self {
        Self {
            shares: NICE_0_WEIGHT,
            idle: false,
            quota: None,
            period: us(PERIOD_DFL_US),
            burst: Duration::ZERO,
            period_start: None,
            runtime: 0,
            used: Duration::ZERO,
            nr_periods: 0,
            nr_throttled: 0,
            nr_bursts: 0,
            throttled_time: Duration::ZERO,
            burst_time: Duration::ZERO,
            throttled_since: None,
            psi: Psi::default(),
        }
    }
}

/// What `cpu.stat` adds for a cgroup with the controller (`cpu_extra_stat_show`).
pub(crate) struct BandwidthStat {
    pub(crate) nr_periods: u64,
    pub(crate) nr_throttled: u64,
    pub(crate) throttled: Duration,
    pub(crate) nr_bursts: u64,
    pub(crate) burst: Duration,
}

impl Bandwidth {
    /// `cpu.weight` (`sched_weight_to_cgroup`).
    pub(crate) fn weight(&self) -> u64 {
        div_round(self.shares * WEIGHT_DFL, NICE_0_WEIGHT).clamp(WEIGHT_MIN, WEIGHT_MAX)
    }

    /// `cpu_weight_write_u64`: `ERANGE` outside 1 to 10000.
    pub(crate) fn set_weight(&mut self, weight: u64) -> Result<(), Errno> {
        if !(WEIGHT_MIN..=WEIGHT_MAX).contains(&weight) {
            return Err(Errno::ERANGE);
        }
        self.shares = div_round(weight * NICE_0_WEIGHT, WEIGHT_DFL);
        Ok(())
    }

    /// `cpu_weight_nice_read_s64`: the nice value whose weight is closest.
    pub(crate) fn weight_nice(&self) -> i64 {
        let mut last = u64::MAX;
        let mut prio = 0;
        while prio < PRIO_TO_WEIGHT.len() {
            let delta = PRIO_TO_WEIGHT[prio].abs_diff(self.shares);
            if delta >= last {
                break;
            }
            last = delta;
            prio += 1;
        }
        prio as i64 - 1 - 20
    }

    /// `cpu_weight_nice_write_s64`: `ERANGE` outside -20 to 19.
    pub(crate) fn set_weight_nice(&mut self, nice: i64) -> Result<(), Errno> {
        if !(-20..=19).contains(&nice) {
            return Err(Errno::ERANGE);
        }
        self.shares = PRIO_TO_WEIGHT[(nice + 20) as usize];
        Ok(())
    }

    pub(crate) fn idle(&self) -> bool {
        self.idle
    }

    pub(crate) fn set_idle(&mut self, idle: bool) {
        self.idle = idle;
    }

    /// `cpu.max`: the quota (none for `max`) and the period, in microseconds.
    pub(crate) fn max(&self) -> (Option<u64>, u64) {
        let quota = self.quota.map(|q| q.as_micros() as u64);
        (quota, self.period.as_micros() as u64)
    }

    pub(crate) fn burst_us(&self) -> u64 {
        self.burst.as_micros() as u64
    }

    /// `tg_set_bandwidth`: `EINVAL` for a quota or period below 1 ms, a period above 1 s, a
    /// quota past what the runtime holds, or a burst above the quota. A new setting starts a
    /// new period with its whole quota, and lets throttled tasks go (`tg_set_cfs_bandwidth`).
    pub(crate) fn set_bandwidth(
        &mut self,
        quota_us: Option<u64>,
        period_us: u64,
        burst_us: u64,
    ) -> Result<(), Errno> {
        let max_usec = u64::MAX / 1000;
        if period_us > max_usec || quota_us.is_some_and(|q| q > max_usec) || burst_us > max_usec {
            return Err(Errno::EINVAL);
        }
        if quota_us.is_some_and(|q| q < MIN_US) || period_us < MIN_US {
            return Err(Errno::EINVAL);
        }
        if period_us > MAX_PERIOD_US {
            return Err(Errno::EINVAL);
        }
        if let Some(quota) = quota_us
            && (quota > MAX_RUNTIME_US || burst_us > quota || burst_us + quota > MAX_RUNTIME_US)
        {
            return Err(Errno::EINVAL);
        }
        self.quota = quota_us.map(us);
        self.period = us(period_us);
        self.burst = us(burst_us);
        self.period_start = None;
        self.runtime = self.quota.map_or(0, |q| q.as_nanos() as i128);
        self.used = Duration::ZERO;
        self.throttled_since = None;
        Ok(())
    }

    /// `do_sched_cfs_period_timer` for the periods that ended by `now`: each refills the
    /// runtime, with what was left carried over up to the burst (`__refill_cfs_bandwidth_runtime`),
    /// and unthrottles. A period that used more than its quota drew on the burst.
    fn refresh(&mut self, now: Duration) {
        let Some(quota) = self.quota else {
            return;
        };
        let start = *self.period_start.get_or_insert(now);
        if now < start + self.period {
            return;
        }
        let periods = (now - start).as_nanos() / self.period.as_nanos();
        let end = start + self.period;
        self.nr_periods += 1;
        if self.used > quota && !self.burst.is_zero() {
            self.nr_bursts += 1;
            self.burst_time += (self.used - quota).min(self.burst);
        }
        if let Some(since) = self.throttled_since.take() {
            self.throttled_time += end.saturating_sub(since);
        }
        let quota_ns = quota.as_nanos() as i128;
        let cap = quota_ns + self.burst.as_nanos() as i128;
        self.runtime = (self.runtime + quota_ns * periods as i128).min(cap);
        self.used = Duration::ZERO;
        let periods = u32::try_from(periods).unwrap_or(u32::MAX);
        self.period_start = Some(start + self.period.saturating_mul(periods));
    }

    /// `account_cfs_rq_runtime`: a task of the subtree ran for `delta`. Returns when the
    /// period ends if the runtime ran out (`throttle_cfs_rq`).
    pub(crate) fn charge(&mut self, delta: Duration, now: Duration) -> Option<Duration> {
        self.quota?;
        self.refresh(now);
        self.used += delta;
        self.runtime -= delta.as_nanos() as i128;
        if self.runtime > 0 {
            return None;
        }
        if self.throttled_since.is_none() {
            self.throttled_since = Some(now);
            self.nr_throttled += 1;
        }
        self.period_start.map(|start| start + self.period)
    }

    /// The time the cgroup was throttled, up to `now`, after the periods that ended.
    fn throttled_until(&mut self, now: Duration) -> Duration {
        self.refresh(now);
        let current = self.throttled_since.map(|since| now.saturating_sub(since));
        self.throttled_time + current.unwrap_or_default()
    }

    /// `cpu_extra_stat_show`.
    pub(crate) fn stat(&mut self, now: Duration) -> BandwidthStat {
        let throttled = self.throttled_until(now);
        BandwidthStat {
            nr_periods: self.nr_periods,
            nr_throttled: self.nr_throttled,
            throttled,
            nr_bursts: self.nr_bursts,
            burst: self.burst_time,
        }
    }

    /// `cpu.pressure`: the throttled time, for some and all of the cgroup's tasks alike.
    pub(crate) fn pressure(&mut self, now: Duration) -> String {
        let stalled = self.throttled_until(now);
        self.psi.update(stalled, now);
        let avg = self.psi.avg;
        pressure(&[("some", avg, stalled), ("full", avg, stalled)])
    }
}

/// Pressure averages over 10, 60 and 300 seconds (`psi_group`'s `avg`), updated as they are
/// read from the stall time so far (`calc_avgs`): each update decays the old average by the
/// time since the last one.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Psi {
    avg: [f64; 3],
    last: Option<(Duration, Duration)>,
}

impl Psi {
    fn update(&mut self, stalled: Duration, now: Duration) {
        if let Some((at, before)) = self.last {
            let dt = now.saturating_sub(at).as_secs_f64();
            if dt <= 0.0 {
                return;
            }
            let pct = (stalled.saturating_sub(before).as_secs_f64() / dt * 100.0).min(100.0);
            for (avg, window) in self.avg.iter_mut().zip([10.0, 60.0, 300.0]) {
                let decay = (-dt / window).exp();
                *avg = *avg * decay + pct * (1.0 - decay);
            }
        }
        self.last = Some((now, stalled));
    }
}

/// `psi_show`: a line per kind, with its averages and its total in microseconds.
pub(crate) fn pressure(lines: &[(&str, [f64; 3], Duration)]) -> String {
    let mut out = String::new();
    for (kind, [a10, a60, a300], total) in lines {
        out += &format!(
            "{kind} avg10={a10:.2} avg60={a60:.2} avg300={a300:.2} total={}\n",
            total.as_micros()
        );
    }
    out
}

/// No stall ever: `some` and `full` at zero.
pub(crate) fn no_pressure() -> String {
    pressure(&[
        ("some", [0.0; 3], Duration::ZERO),
        ("full", [0.0; 3], Duration::ZERO),
    ])
}

/// A device as `io.stat` and `io.max` name it: major and minor.
pub(crate) type Dev = (u32, u32);

/// What `io.stat` counts for a device (`blkg_iostat`); discards never happen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct IoStat {
    pub(crate) rbytes: u64,
    pub(crate) wbytes: u64,
    pub(crate) rios: u64,
    pub(crate) wios: u64,
}

impl IoStat {
    fn add(&mut self, other: &IoStat) {
        self.rbytes += other.rbytes;
        self.wbytes += other.wbytes;
        self.rios += other.rios;
        self.wios += other.wios;
    }
}

/// `io.max` of a device: `rbps`, `wbps`, `riops`, `wiops`; none for `max`.
pub(crate) type IoLimits = [Option<u64>; 4];
const IO_KEYS: [&str; 4] = ["rbps", "wbps", "riops", "wiops"];

/// The `io` controller's state of a cgroup: what its own tasks did, and its limits.
#[derive(Clone, Debug, Default)]
pub(crate) struct Io {
    stat: BTreeMap<Dev, IoStat>,
    max: BTreeMap<Dev, IoLimits>,
}

impl Io {
    /// A read (or write) of `bytes` through a file of `dev`.
    pub(crate) fn charge(&mut self, dev: Dev, bytes: u64, write: bool) {
        let stat = self.stat.entry(dev).or_default();
        match write {
            true => (stat.wbytes, stat.wios) = (stat.wbytes + bytes, stat.wios + 1),
            false => (stat.rbytes, stat.rios) = (stat.rbytes + bytes, stat.rios + 1),
        }
    }

    /// Adds what this cgroup's tasks did to `total`.
    pub(crate) fn sum_into(&self, total: &mut BTreeMap<Dev, IoStat>) {
        for (dev, stat) in &self.stat {
            total.entry(*dev).or_default().add(stat);
        }
    }

    /// `tg_set_limit`: `MAJ:MIN` and `key=value` pairs, `max` for no limit; `EINVAL` for a
    /// malformed request or an unknown key, `ERANGE` for 0. Limits all `max` drop the device.
    pub(crate) fn set_max(&mut self, text: &str) -> Result<(), Errno> {
        let mut tokens = text.split_whitespace();
        let dev = parse_dev(tokens.next().ok_or(Errno::EINVAL)?)?;
        let mut limits = self.max.get(&dev).copied().unwrap_or_default();
        for token in tokens {
            let (key, value) = token.split_once('=').ok_or(Errno::EINVAL)?;
            let value = match value {
                "max" => None,
                value => Some(value.parse::<u64>().map_err(|_| Errno::EINVAL)?),
            };
            if value == Some(0) {
                return Err(Errno::ERANGE);
            }
            let at = IO_KEYS
                .iter()
                .position(|k| *k == key)
                .ok_or(Errno::EINVAL)?;
            // The iops limits are `unsigned int`.
            limits[at] = match at {
                2 | 3 => value.map(|v| v.min(u64::from(u32::MAX))),
                _ => value,
            };
        }
        match limits.iter().all(Option::is_none) {
            true => self.max.remove(&dev),
            false => self.max.insert(dev, limits),
        };
        Ok(())
    }

    /// The controller went (`kill_css`): its limits go with it.
    pub(crate) fn clear_max(&mut self) {
        self.max.clear();
    }

    /// `tg_prfill_limit`: a line per device with a limit.
    pub(crate) fn show_max(&self) -> String {
        let mut out = String::new();
        for ((major, minor), limits) in &self.max {
            out += &format!("{major}:{minor}");
            for (key, limit) in IO_KEYS.iter().zip(limits) {
                match limit {
                    Some(v) => out += &format!(" {key}={v}"),
                    None => out += &format!(" {key}=max"),
                }
            }
            out.push('\n');
        }
        out
    }
}

/// `blkcg_print_one_stat`: a line per device that saw any I/O.
pub(crate) fn show_stat(total: &BTreeMap<Dev, IoStat>) -> String {
    let mut out = String::new();
    for ((major, minor), s) in total.iter().filter(|(_, s)| **s != IoStat::default()) {
        out += &format!(
            "{major}:{minor} rbytes={} wbytes={} rios={} wios={} dbytes=0 dios=0\n",
            s.rbytes, s.wbytes, s.rios, s.wios
        );
    }
    out
}

/// `MAJ:MIN` (`blkg_conf_open_bdev`'s `sscanf("%u:%u")`).
fn parse_dev(text: &str) -> Result<Dev, Errno> {
    let (major, minor) = text.split_once(':').ok_or(Errno::EINVAL)?;
    let num = |s: &str| s.parse::<u32>().map_err(|_| Errno::EINVAL);
    Ok((num(major)?, num(minor)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_convert_as_the_scheduler_does() {
        let mut b = Bandwidth::default();
        assert_eq!((b.weight(), b.weight_nice()), (100, 0));
        b.set_weight_nice(-20).unwrap();
        assert_eq!((b.weight(), b.weight_nice()), (8668, -20));
        b.set_weight(1).unwrap();
        assert_eq!(b.weight_nice(), 19);
        assert_eq!(b.set_weight(0), Err(Errno::ERANGE));
        assert_eq!(b.set_weight_nice(20), Err(Errno::ERANGE));
    }

    #[test]
    fn a_quota_throttles_until_the_period_ends_and_counts_it() {
        let ms = Duration::from_millis;
        let mut b = Bandwidth::default();
        assert_eq!(b.set_bandwidth(Some(999), 100_000, 0), Err(Errno::EINVAL));
        assert_eq!(
            b.set_bandwidth(Some(1000), 2_000_000, 0),
            Err(Errno::EINVAL)
        );
        b.set_bandwidth(Some(10_000), 100_000, 0).unwrap();
        assert_eq!(b.max(), (Some(10_000), 100_000));

        assert_eq!(b.charge(ms(5), ms(1000)), None);
        assert_eq!(b.charge(ms(5), ms(1010)), Some(ms(1100)));
        assert_eq!(b.charge(ms(1), ms(1020)), Some(ms(1100)));
        let stat = b.stat(ms(1050));
        assert_eq!((stat.nr_throttled, stat.throttled), (1, ms(40)));
        // The next period refills and lets it go.
        assert_eq!(b.charge(ms(1), ms(1150)), None);
        let stat = b.stat(ms(1150));
        assert_eq!((stat.nr_periods, stat.throttled), (1, ms(90)));
        assert_eq!((stat.nr_bursts, stat.burst), (0, ms(0)), "no burst allowed");
    }

    #[test]
    fn io_max_keeps_limits_per_device() {
        let mut io = Io::default();
        io.set_max("8:0 rbps=1048576 wiops=120").unwrap();
        assert_eq!(
            io.show_max(),
            "8:0 rbps=1048576 wbps=max riops=max wiops=120\n"
        );
        assert_eq!(io.set_max("8:0 rbps=0"), Err(Errno::ERANGE));
        assert_eq!(io.set_max("8:0 nope=1"), Err(Errno::EINVAL));
        assert_eq!(io.set_max("8"), Err(Errno::EINVAL));
        io.set_max("8:0 rbps=max wiops=max").unwrap();
        assert_eq!(io.show_max(), "");
    }

    #[test]
    fn pressure_averages_decay_towards_the_stall_share() {
        let mut psi = Psi::default();
        let s = Duration::from_secs;
        psi.update(s(0), s(0));
        psi.update(s(10), s(10));
        assert!(psi.avg[0] > 60.0 && psi.avg[0] < 70.0, "{:?}", psi.avg);
        assert_eq!(
            no_pressure(),
            "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n\
             full avg10=0.00 avg60=0.00 avg300=0.00 total=0\n"
        );
    }
}
