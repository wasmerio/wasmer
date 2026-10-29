//! The scheduler interface, after Linux's `kernel/sched/syscalls.c` and the priority half of
//! `kernel/sys.c`: policies and their parameters, nice values, utilization clamps, CPU affinity,
//! the timer slack, `sched_yield` and `getcpu`.
//!
//! Scheduling is advisory. Guest threads run on host threads or coroutines that the host
//! schedules as it sees fit, so no policy, priority, nice value or affinity changes when or where
//! a guest runs, and the real-time and deadline policies give no real-time guarantees. What the
//! kernel does keep is every attribute, per task as on Linux, validated, inherited and reported
//! exactly as Linux does, under Linux's permission (`CAP_SYS_NICE`, `RLIMIT_NICE`,
//! `RLIMIT_RTPRIO`) and deadline admission rules.

use std::{future::poll_fn, task::Poll, time::Duration};

use wasixv2_abi::{
    ConstSchedParamPtr, GuestLayout, KernelTimespecPtr, Pid, PrioWhich, RlimitResource, SchedAttr,
    SchedAttrPtr, SchedFlags, SchedGetattrFlags, SchedParam, SchedParamPtr, SchedPolicy, U32Ptr,
    UlongArray,
};
use wasixv2_syscalls::{Caller, Errno, SysResult, TaskId, read_value, write_value};
use wasixv2_vfs::Cap;

use crate::{Kernel, State, mm::PAGE_SIZE, sync::MutexExt, time};

/// `MAX_RT_PRIO`: real-time priorities run from 1 to `MAX_RT_PRIO - 1`.
const MAX_RT_PRIO: u32 = 100;
const MIN_NICE: i32 = -20;
const MAX_NICE: i32 = 19;
/// `CONFIG_HZ`: time slices are reported in whole ticks.
const HZ: u64 = 1000;
/// `RR_TIMESLICE`: the `SCHED_RR` quantum.
const RR_TIMESLICE: Duration = Duration::from_millis(100);
/// `normalized_sysctl_sched_base_slice`, before it is scaled by the CPU count.
const BASE_SLICE_NS: u64 = 700_000;
/// What a fair task's requested slice (`sched_attr.sched_runtime`) is clamped to.
const MIN_SLICE_NS: u64 = 100_000;
const MAX_SLICE_NS: u64 = 100_000_000;
/// The default `timer_slack_ns`.
const TIMER_SLACK_NS: u64 = 50_000;
/// `SCHED_ATTR_SIZE_VER0` (without the utilization clamps) and `SCHED_ATTR_SIZE_VER1`.
const ATTR_SIZE_VER0: u32 = 48;
const ATTR_SIZE_VER1: u32 = SchedAttr::SIZE as u32;
/// `SCHED_CAPACITY_SCALE`: the top of the utilization clamp range.
const CAPACITY_SCALE: u32 = 1024;
/// `sysctl_sched_uclamp_util_min_rt_default`: real-time tasks run boosted to the top.
const UCLAMP_MIN_RT_DEFAULT: u32 = CAPACITY_SCALE;
/// `DL_SCALE`: a deadline runtime is at least 2^`DL_SCALE` ns.
const DL_SCALE: u32 = 10;
/// `sysctl_sched_dl_period_min` and `sysctl_sched_dl_period_max`, in nanoseconds.
const DL_PERIOD_MIN_NS: u64 = 100 * 1000;
const DL_PERIOD_MAX_NS: u64 = (1 << 22) * 1000;
/// `BW_SHIFT`: bandwidths are fixed-point fractions of a CPU with this many fraction bits.
const BW_SHIFT: u32 = 20;
/// `sched_rt_period_us` and `sched_rt_runtime_us`: deadline tasks may take 95% of each CPU.
const RT_PERIOD_NS: u64 = 1_000_000_000;
const RT_RUNTIME_NS: u64 = 950_000_000;
/// The fair server's reservation on every CPU (`fair_server_init`): 50 ms each second.
const FAIR_SERVER_RUNTIME_NS: u64 = 50_000_000;
/// `SCHED_DL_FLAGS` a user can pass: what a deadline task keeps with its parameters.
const DL_FLAGS: u64 = SchedFlags::SCHED_FLAG_RECLAIM.0 | SchedFlags::SCHED_FLAG_DL_OVERRUN.0;
/// `SCHED_FLAG_SUGOV`: for the kernel's frequency governor threads only.
const FLAG_SUGOV: u64 = 0x1000_0000;
/// Bytes per guest `cpu_set_t` word (`unsigned long`).
const MASK_WORD: u32 = 4;

/// What Linux keeps on `task_struct` for scheduling, per task.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Sched {
    /// Never carries `SCHED_RESET_ON_FORK`; that is `reset_on_fork`.
    policy: SchedPolicy,
    reset_on_fork: bool,
    /// Kept under every policy; it only counts for the fair ones.
    nice: i32,
    rt_priority: u32,
    /// `se.slice`, and whether the task asked for it (`se.custom_slice`).
    slice: u64,
    custom_slice: bool,
    dl: Deadline,
    /// `uclamp_req`: the requested minimum and maximum utilization.
    uclamp: [Clamp; 2],
    /// CPUs the task may run on, one bit each.
    affinity: Vec<u32>,
    timer_slack: u64,
    /// What `PR_SET_TIMERSLACK` with 0 restores: the parent's slack at fork.
    default_timer_slack: u64,
}

/// `sched_dl_entity`'s parameters.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Deadline {
    runtime: u64,
    deadline: u64,
    /// The period, or the deadline when none was given.
    period: u64,
    flags: u64,
    /// When the parameters took effect: the start of the first period.
    start: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Clamp {
    value: u32,
    /// Set by `sched_setattr` rather than defaulted from the policy.
    user_defined: bool,
}

const UCLAMP_MIN: usize = 0;
const UCLAMP_MAX: usize = 1;

/// `uclamp_none`.
fn uclamp_none(id: usize) -> u32 {
    match id {
        UCLAMP_MIN => 0,
        _ => CAPACITY_SCALE,
    }
}

impl Default for Sched {
    /// `init_task`'s attributes.
    fn default() -> Self {
        Self {
            policy: SchedPolicy::SCHED_NORMAL,
            reset_on_fork: false,
            nice: 0,
            rt_priority: 0,
            slice: base_slice(),
            custom_slice: false,
            dl: Deadline::default(),
            uclamp: [UCLAMP_MIN, UCLAMP_MAX].map(|id| Clamp {
                value: uclamp_none(id),
                user_defined: false,
            }),
            affinity: online_mask(),
            timer_slack: TIMER_SLACK_NS,
            default_timer_slack: TIMER_SLACK_NS,
        }
    }
}

/// What deciding a `sched_setattr` needs to know beyond the target's own attributes.
struct Admission {
    /// The target's `RLIMIT_NICE` and `RLIMIT_RTPRIO`.
    nice_limit: u64,
    rtprio_limit: u64,
    /// Whether the caller has `CAP_SYS_NICE`.
    capable: bool,
    /// Whether the caller's effective uid is the target's real or effective uid
    /// (`check_same_owner`).
    same_owner: bool,
    /// The bandwidth every other deadline task holds.
    dl_others: u64,
    now: Duration,
}

fn is_fair(policy: SchedPolicy) -> bool {
    matches!(policy, SchedPolicy::SCHED_NORMAL | SchedPolicy::SCHED_BATCH)
}

fn is_rt(policy: SchedPolicy) -> bool {
    matches!(policy, SchedPolicy::SCHED_FIFO | SchedPolicy::SCHED_RR)
}

fn is_dl(policy: SchedPolicy) -> bool {
    policy == SchedPolicy::SCHED_DEADLINE
}

/// `valid_policy`. There is no `sched_ext` scheduler, so `SCHED_EXT` is not one.
fn valid_policy(policy: SchedPolicy) -> bool {
    is_fair(policy) || is_rt(policy) || is_dl(policy) || policy == SchedPolicy::SCHED_IDLE
}

fn has(flags: SchedFlags, flag: SchedFlags) -> bool {
    flags.0 & flag.0 != 0
}

/// `is_nice_reduction`: whether `RLIMIT_NICE` allows `nice`.
fn nice_allowed(nice: i32, limit: u64) -> bool {
    (20 - nice) as u64 <= limit
}

/// `to_ratio`: `runtime` per `period` as a `BW_SHIFT` fixed-point fraction.
fn to_ratio(period: u64, runtime: u64) -> u64 {
    match period {
        0 => 0,
        _ => ((u128::from(runtime) << BW_SHIFT) / u128::from(period)) as u64,
    }
}

/// `__checkparam_dl`: runtime <= deadline <= period, with a period within the sysctl bounds.
fn deadline_valid(attr: &SchedAttr) -> bool {
    if attr.sched_flags.0 & FLAG_SUGOV != 0 {
        return true;
    }
    if attr.sched_deadline == 0 || attr.sched_runtime < 1 << DL_SCALE {
        return false;
    }
    if (attr.sched_deadline | attr.sched_period) & (1 << 63) != 0 {
        return false;
    }
    let period = match attr.sched_period {
        0 => attr.sched_deadline,
        period => period,
    };
    period >= attr.sched_deadline
        && attr.sched_deadline >= attr.sched_runtime
        && (DL_PERIOD_MIN_NS..=DL_PERIOD_MAX_NS).contains(&period)
}

/// CPUs the host offers (`nr_cpu_ids`).
/// The pid namespace of the caller (`task_active_pid_ns(current)`), from the task table its
/// caller holds.
fn caller_pid_ns<'a>(state: &'a State, caller: &Caller<'_>) -> &'a crate::ns::PidNs {
    &state.tasks[&caller.task()].process.pid_ns
}

pub(crate) fn cpus() -> u32 {
    std::thread::available_parallelism().map_or(1, |n| n.get().min(1024) as u32)
}

/// Every online CPU.
fn online_mask() -> Vec<u32> {
    let cpus = cpus();
    let mut mask = vec![0; cpus.div_ceil(32) as usize];
    for cpu in 0..cpus {
        mask[(cpu / 32) as usize] |= 1 << (cpu % 32);
    }
    mask
}

/// `cpumask_size()`: bytes of a mask covering every possible CPU.
fn mask_size() -> u32 {
    cpus().div_ceil(32) * MASK_WORD
}

/// `sysctl_sched_base_slice`: the normalized slice scaled by `1 + ilog2(cpus)`, for at most 8
/// CPUs (`SCHED_TUNABLESCALING_LOG`).
fn base_slice() -> u64 {
    BASE_SLICE_NS * u64::from(1 + cpus().min(8).ilog2())
}

/// What deadline tasks may hold in all (`dl_bw->bw` scaled by the CPUs), less what the fair
/// servers reserve.
fn dl_capacity() -> u64 {
    let cpus = u64::from(cpus());
    let fair = to_ratio(RT_PERIOD_NS, FAIR_SERVER_RUNTIME_NS);
    (to_ratio(RT_PERIOD_NS, RT_RUNTIME_NS) - fair) * cpus
}

/// A time in whole scheduler ticks, as `jiffies_to_timespec64(NS_TO_JIFFIES(ns))`.
fn in_ticks(ns: u64) -> Duration {
    let tick = 1_000_000_000 / HZ;
    Duration::from_nanos(ns / tick * tick)
}

impl Sched {
    fn rt(&self) -> bool {
        is_rt(self.policy)
    }

    fn dl(&self) -> bool {
        is_dl(self.policy)
    }

    /// `p->dl.dl_bw`, which a task keeps after it leaves `SCHED_DEADLINE`.
    fn dl_bw(&self) -> u64 {
        to_ratio(self.dl.period, self.dl.runtime)
    }

    /// The bandwidth the task holds against the deadline capacity.
    fn dl_held(&self) -> u64 {
        match self.dl() {
            true => self.dl_bw(),
            false => 0,
        }
    }

    pub(crate) fn policy(&self) -> SchedPolicy {
        self.policy
    }

    pub(crate) fn nice(&self) -> i32 {
        self.nice
    }

    /// The real-time priority, 0 for other policies (`rt_priority`).
    pub(crate) fn rt_priority(&self) -> u32 {
        self.rt_priority
    }

    /// `task_prio`: what `/proc/<pid>/stat` reports as the priority, `nice + 20` for fair
    /// policies, `-1 - rt_priority` for real-time ones and -101 for deadline tasks.
    pub(crate) fn prio(&self) -> i64 {
        match () {
            _ if self.dl() => -101,
            _ if self.rt() => -1 - i64::from(self.rt_priority),
            _ => i64::from(self.nice) + 20,
        }
    }

    /// The CPUs the task may run on, one bit each in 32-bit words.
    pub(crate) fn affinity(&self) -> &[u32] {
        &self.affinity
    }

    /// `PR_GET_TIMERSLACK`.
    pub(crate) fn timer_slack(&self) -> u64 {
        self.timer_slack
    }

    /// `PR_SET_TIMERSLACK`: 0 restores the default, and real-time and deadline tasks have none.
    pub(crate) fn set_timer_slack(&mut self, slack: u64) {
        if self.rt() || self.dl() {
            return;
        }
        self.timer_slack = match slack {
            0 => self.default_timer_slack,
            slack => slack,
        };
    }

    /// `sched_fork`: what a new task, thread or process, starts with. A deadline task cannot
    /// fork unless it resets on fork.
    pub(crate) fn for_child(&self) -> Result<Self, Errno> {
        let mut child = self.clone();
        child.default_timer_slack = self.timer_slack;
        if child.reset_on_fork {
            if child.rt() || child.dl() {
                child.policy = SchedPolicy::SCHED_NORMAL;
                child.nice = 0;
                child.rt_priority = 0;
                child.timer_slack = child.default_timer_slack;
            } else if child.nice < 0 {
                child.nice = 0;
            }
            child.custom_slice = false;
            child.slice = base_slice();
            child.reset_on_fork = false;
            for (id, clamp) in child.uclamp.iter_mut().enumerate() {
                *clamp = Clamp {
                    value: uclamp_none(id),
                    user_defined: false,
                };
            }
        }
        if child.dl() {
            return Err(Errno::EAGAIN);
        }
        // `uclamp_post_fork`.
        if child.rt() && !child.uclamp[UCLAMP_MIN].user_defined {
            child.uclamp[UCLAMP_MIN].value = UCLAMP_MIN_RT_DEFAULT;
        }
        Ok(child)
    }

    /// `sched_getscheduler`'s result.
    fn policy_word(&self) -> i32 {
        match self.reset_on_fork {
            true => self.policy.0 | SchedPolicy::SCHED_RESET_ON_FORK.0,
            false => self.policy.0,
        }
    }

    /// `sched_getparam`: only real-time tasks have a priority.
    fn param(&self) -> SchedParam {
        SchedParam {
            sched_priority: match self.rt() {
                true => self.rt_priority as i32,
                false => 0,
            },
        }
    }

    /// `get_params`. `now` asks for a deadline task's current runtime and absolute deadline
    /// (`SCHED_GETATTR_FLAG_DL_DYNAMIC`). No budget is ever consumed, so the runtime is full and
    /// the deadline is that of the current period.
    fn params(&self, attr: &mut SchedAttr, now: Option<Duration>) {
        if self.dl() {
            attr.sched_priority = self.rt_priority;
            attr.sched_runtime = self.dl.runtime;
            attr.sched_deadline = match now {
                Some(now) => {
                    let period = self.dl.period.max(1);
                    let elapsed = now.saturating_sub(self.dl.start).as_nanos() as u64;
                    let start = self.dl.start.as_nanos() as u64 + elapsed / period * period;
                    start + self.dl.deadline
                }
                None => self.dl.deadline,
            };
            attr.sched_period = self.dl.period;
            attr.sched_flags = SchedFlags((attr.sched_flags.0 & !DL_FLAGS) | self.dl.flags);
        } else if self.rt() {
            attr.sched_priority = self.rt_priority;
        } else {
            attr.sched_nice = self.nice;
            attr.sched_runtime = self.slice;
        }
    }

    /// `sched_getattr`'s attributes.
    fn attr(&self, now: Option<Duration>) -> SchedAttr {
        let mut attr = SchedAttr {
            size: ATTR_SIZE_VER1,
            sched_policy: self.policy.0 as u32,
            ..SchedAttr::default()
        };
        if self.reset_on_fork {
            attr.sched_flags = SchedFlags::SCHED_FLAG_RESET_ON_FORK;
        }
        self.params(&mut attr, now);
        attr.sched_flags.0 &= SchedFlags::SCHED_FLAG_ALL.0;
        attr.sched_util_min = self.uclamp[UCLAMP_MIN].value;
        attr.sched_util_max = self.uclamp[UCLAMP_MAX].value;
        attr
    }

    /// `_sched_setscheduler`: the attributes `sched_setscheduler` (`policy`) and
    /// `sched_setparam` (`None`) ask for. Returns them and whether to keep the policy.
    fn legacy_attr(&self, policy: Option<SchedPolicy>, priority: i32) -> (SchedAttr, bool) {
        let mut attr = SchedAttr {
            sched_priority: priority as u32,
            sched_nice: self.nice,
            ..SchedAttr::default()
        };
        if self.custom_slice {
            attr.sched_runtime = self.slice;
        }
        if let Some(policy) = policy {
            let reset = SchedPolicy::SCHED_RESET_ON_FORK.0;
            if policy.0 & reset != 0 {
                attr.sched_flags = SchedFlags::SCHED_FLAG_RESET_ON_FORK;
            }
            attr.sched_policy = (policy.0 & !reset) as u32;
        }
        (attr, policy.is_none())
    }

    /// `sched_rr_get_interval`: the `SCHED_RR` quantum, a fair task's slice, and 0 (no limit)
    /// for `SCHED_FIFO` and `SCHED_DEADLINE`.
    fn rr_interval(&self) -> Duration {
        match self.policy {
            SchedPolicy::SCHED_RR => RR_TIMESLICE,
            SchedPolicy::SCHED_FIFO | SchedPolicy::SCHED_DEADLINE => Duration::ZERO,
            _ => in_ticks(self.slice),
        }
    }

    /// `__sched_setscheduler` for a user request. `keep_policy` is `sched_setparam` and
    /// `SCHED_FLAG_KEEP_POLICY`, which keep the policy and the reset-on-fork flag.
    fn set(&mut self, attr: &SchedAttr, keep_policy: bool, adm: &Admission) -> Result<(), Errno> {
        let flags = attr.sched_flags;
        let (policy, reset_on_fork) = match keep_policy {
            true => (self.policy, self.reset_on_fork),
            false => {
                let policy = SchedPolicy(attr.sched_policy as i32);
                if !valid_policy(policy) {
                    return Err(Errno::EINVAL);
                }
                (policy, has(flags, SchedFlags::SCHED_FLAG_RESET_ON_FORK))
            }
        };
        if flags.0 & !(SchedFlags::SCHED_FLAG_ALL.0 | FLAG_SUGOV) != 0 {
            return Err(Errno::EINVAL);
        }
        if attr.sched_priority > MAX_RT_PRIO - 1 {
            return Err(Errno::EINVAL);
        }
        if (is_dl(policy) && !deadline_valid(attr)) || is_rt(policy) != (attr.sched_priority != 0) {
            return Err(Errno::EINVAL);
        }
        if self.needs_privilege(attr, policy, reset_on_fork, adm) && !adm.capable {
            return Err(Errno::EPERM);
        }
        if flags.0 & FLAG_SUGOV != 0 {
            return Err(Errno::EINVAL);
        }
        if has(flags, SchedFlags::SCHED_FLAG_UTIL_CLAMP) {
            self.uclamp_validate(attr)?;
        }
        if policy == self.policy && !self.changes(attr, policy) {
            self.reset_on_fork = reset_on_fork;
            return Ok(());
        }
        // A deadline task must be allowed on every CPU of its root domain.
        if is_dl(policy) && self.affinity != online_mask() {
            return Err(Errno::EPERM);
        }
        if (is_dl(policy) || self.dl()) && self.dl_overflow(attr, policy, adm) {
            return Err(Errno::EBUSY);
        }
        self.reset_on_fork = reset_on_fork;
        if !has(flags, SchedFlags::SCHED_FLAG_KEEP_PARAMS) {
            self.set_params(attr, policy, adm.now);
        }
        self.set_uclamp(attr);
        Ok(())
    }

    /// `user_check_sched_setscheduler`: whether the change needs `CAP_SYS_NICE`.
    fn needs_privilege(
        &self,
        attr: &SchedAttr,
        policy: SchedPolicy,
        reset_on_fork: bool,
        adm: &Admission,
    ) -> bool {
        if is_fair(policy)
            && attr.sched_nice < self.nice
            && !nice_allowed(attr.sched_nice, adm.nice_limit)
        {
            return true;
        }
        if is_rt(policy) {
            if policy != self.policy && adm.rtprio_limit == 0 {
                return true;
            }
            if attr.sched_priority > self.rt_priority
                && u64::from(attr.sched_priority) > adm.rtprio_limit
            {
                return true;
            }
        }
        // Unprivileged deadline tasks are not allowed at all.
        if is_dl(policy) {
            return true;
        }
        // SCHED_IDLE counts as nice 20.
        if self.policy == SchedPolicy::SCHED_IDLE
            && policy != SchedPolicy::SCHED_IDLE
            && !nice_allowed(self.nice, adm.nice_limit)
        {
            return true;
        }
        if self.reset_on_fork && !reset_on_fork {
            return true;
        }
        // Another user's tasks cannot change.
        !adm.same_owner
    }

    /// Whether `attr` changes anything under the same `policy`.
    fn changes(&self, attr: &SchedAttr, policy: SchedPolicy) -> bool {
        if is_fair(policy) && (attr.sched_nice != self.nice || attr.sched_runtime != self.slice) {
            return true;
        }
        if is_rt(policy) && attr.sched_priority != self.rt_priority {
            return true;
        }
        // `dl_param_changed`.
        if is_dl(policy)
            && (self.dl.runtime != attr.sched_runtime
                || self.dl.deadline != attr.sched_deadline
                || self.dl.period != attr.sched_period
                || self.dl.flags != attr.sched_flags.0 & DL_FLAGS)
        {
            return true;
        }
        has(attr.sched_flags, SchedFlags::SCHED_FLAG_UTIL_CLAMP)
    }

    /// `sched_dl_overflow`: whether entering or changing `SCHED_DEADLINE` would take more
    /// bandwidth than is left. Leaving it never fails.
    fn dl_overflow(&self, attr: &SchedAttr, policy: SchedPolicy, adm: &Admission) -> bool {
        if attr.sched_flags.0 & FLAG_SUGOV != 0 || !is_dl(policy) {
            return false;
        }
        let period = match attr.sched_period {
            0 => attr.sched_deadline,
            period => period,
        };
        let new = to_ratio(period, attr.sched_runtime);
        if self.dl() && new == self.dl_bw() {
            return false;
        }
        adm.dl_others.saturating_add(new) > dl_capacity()
    }

    /// `__setscheduler_params`.
    fn set_params(&mut self, attr: &SchedAttr, policy: SchedPolicy, now: Duration) {
        self.policy = policy;
        if is_dl(policy) {
            self.dl = Deadline {
                runtime: attr.sched_runtime,
                deadline: attr.sched_deadline,
                period: match attr.sched_period {
                    0 => attr.sched_deadline,
                    period => period,
                },
                flags: attr.sched_flags.0 & DL_FLAGS,
                start: now,
            };
        } else if is_fair(policy) {
            // `__setparam_fair`.
            self.nice = attr.sched_nice;
            self.custom_slice = attr.sched_runtime != 0;
            self.slice = match attr.sched_runtime {
                0 => base_slice(),
                runtime => runtime.clamp(MIN_SLICE_NS, MAX_SLICE_NS),
            };
        }
        if self.rt() || self.dl() {
            self.timer_slack = 0;
        } else if self.timer_slack == 0 {
            self.timer_slack = self.default_timer_slack;
        }
        self.rt_priority = attr.sched_priority;
    }

    /// `uclamp_validate`. -1 asks for the default.
    fn uclamp_validate(&self, attr: &SchedAttr) -> Result<(), Errno> {
        let in_range = |value: u32| value == u32::MAX || value <= CAPACITY_SCALE;
        let mut min = self.uclamp[UCLAMP_MIN].value;
        let mut max = self.uclamp[UCLAMP_MAX].value;
        if has(attr.sched_flags, SchedFlags::SCHED_FLAG_UTIL_CLAMP_MIN) {
            min = attr.sched_util_min;
            if !in_range(min) {
                return Err(Errno::EINVAL);
            }
        }
        if has(attr.sched_flags, SchedFlags::SCHED_FLAG_UTIL_CLAMP_MAX) {
            max = attr.sched_util_max;
            if !in_range(max) {
                return Err(Errno::EINVAL);
            }
        }
        if min != u32::MAX && max != u32::MAX && min > max {
            return Err(Errno::EINVAL);
        }
        Ok(())
    }

    /// `__setscheduler_uclamp`: clamps nobody asked for follow the policy; requested ones stay.
    fn set_uclamp(&mut self, attr: &SchedAttr) {
        let flags = attr.sched_flags;
        let requested = has(flags, SchedFlags::SCHED_FLAG_UTIL_CLAMP);
        let asks = [
            (SchedFlags::SCHED_FLAG_UTIL_CLAMP_MIN, attr.sched_util_min),
            (SchedFlags::SCHED_FLAG_UTIL_CLAMP_MAX, attr.sched_util_max),
        ];
        let rt = self.rt();
        for (id, (flag, value)) in asks.into_iter().enumerate() {
            let clamp = &mut self.uclamp[id];
            let reset =
                (!requested && !clamp.user_defined) || (has(flags, flag) && value == u32::MAX);
            if reset {
                clamp.value = match rt && id == UCLAMP_MIN {
                    true => UCLAMP_MIN_RT_DEFAULT,
                    false => uclamp_none(id),
                };
                clamp.user_defined = false;
            }
            if has(flags, flag) && value != u32::MAX {
                *clamp = Clamp {
                    value,
                    user_defined: true,
                };
            }
        }
    }
}

/// Priority range of `policy`, or `None` if it is unknown.
fn priority_range(policy: SchedPolicy) -> Option<(i32, i32)> {
    match policy {
        SchedPolicy::SCHED_FIFO | SchedPolicy::SCHED_RR => Some((1, MAX_RT_PRIO as i32 - 1)),
        SchedPolicy::SCHED_NORMAL
        | SchedPolicy::SCHED_BATCH
        | SchedPolicy::SCHED_IDLE
        | SchedPolicy::SCHED_DEADLINE
        | SchedPolicy::SCHED_EXT => Some((0, 0)),
        _ => None,
    }
}

impl Kernel {
    /// Whether task `id` can still be named: not an exited thread, and not part of a reaped
    /// process. A zombie's leader stays until it is reaped.
    fn nameable(&self, state: &State, id: TaskId) -> bool {
        let Some(task) = state.tasks.get(&id) else {
            return false;
        };
        let process = &task.process;
        !task.thread.exited()
            && (id == process.pid || !process.group.exiting())
            // ---- proc ----
            && self.procs.find(process.pid.0).is_some()
    }

    /// `find_process_by_pid`: the task `pid` names in the caller's pid namespace (a thread id,
    /// so any thread), or the caller for 0.
    fn find_task(&self, state: &State, caller: &Caller<'_>, pid: Pid) -> Result<TaskId, Errno> {
        let id = match pid {
            0 => return Ok(caller.task()),
            pid => TaskId(caller_pid_ns(state, caller).find(pid).ok_or(Errno::ESRCH)?),
        };
        match self.nameable(state, id) {
            true => Ok(id),
            false => Err(Errno::ESRCH),
        }
    }

    /// Runs `f` on the scheduling state of the task `pid` names.
    fn with_sched<R>(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        f: impl FnOnce(&mut Sched) -> R,
    ) -> Result<R, Errno> {
        let mut state = self.state.lock_or_recover();
        let id = self.find_task(&state, caller, pid)?;
        let task = state.tasks.get_mut(&id).expect("found above");
        let result = f(&mut task.sched);
        // `cpu.stat`'s `nice_usec` counts the user time of niced tasks.
        task.css.set_nice(task.sched.nice > 0);
        Ok(result)
    }

    /// Applies a `sched_setscheduler`, `sched_setparam` or `sched_setattr` request to the task
    /// `pid` names. `request` builds the attributes from the target's current ones.
    fn sched_set(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        request: impl FnOnce(&Sched) -> (SchedAttr, bool),
    ) -> SysResult<i32> {
        let cred = self.cred(caller);
        let mut state = self.state.lock_or_recover();
        let id = self.find_task(&state, caller, pid)?;
        let dl_others = state
            .tasks
            .iter()
            .filter(|&(&other, task)| {
                other != id && task.sched.dl() && self.nameable(&state, other)
            })
            .map(|(_, task)| task.sched.dl_held())
            .sum();
        let task = state.tasks.get_mut(&id).expect("found above");
        let adm = {
            let resources = task.process.resources.lock_or_recover();
            Admission {
                nice_limit: resources.soft(RlimitResource::RLIMIT_NICE),
                rtprio_limit: resources.soft(RlimitResource::RLIMIT_RTPRIO),
                // `user_check_sched_setscheduler`: `capable(CAP_SYS_NICE)`.
                capable: cred.capable_init(Cap::SysNice),
                same_owner: cred.same_owner(&task.process.cred()),
                dl_others,
                now: self.host.monotonic(),
            }
        };
        let (attr, keep_policy) = request(&task.sched);
        task.sched.set(&attr, keep_policy, &adm)?;
        Ok(0)
    }

    /// Lets other host work run before returning.
    pub(crate) async fn sys_sched_yield(&self) -> SysResult<i32> {
        let mut yielded = false;
        poll_fn(|cx| {
            if yielded {
                return Poll::Ready(());
            }
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        })
        .await;
        Ok(0)
    }

    pub(crate) fn sys_sched_getscheduler(&self, caller: &Caller<'_>, pid: Pid) -> SysResult<i32> {
        if pid < 0 {
            return Err(Errno::EINVAL.into());
        }
        Ok(self.with_sched(caller, pid, |sched| sched.policy_word())?)
    }

    pub(crate) async fn sys_sched_getparam(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        param: SchedParamPtr,
    ) -> SysResult<i32> {
        if param.is_null() || pid < 0 {
            return Err(Errno::EINVAL.into());
        }
        let value = self.with_sched(caller, pid, |sched| sched.param())?;
        write_value(caller.memory(), param, &value).await?;
        Ok(0)
    }

    /// `sched_setscheduler`, and `sched_setparam` for a `policy` of `None`.
    pub(crate) async fn sys_sched_setscheduler(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        policy: Option<SchedPolicy>,
        param: ConstSchedParamPtr,
    ) -> SysResult<i32> {
        if policy.is_some_and(|policy| policy.0 < 0) || param.is_null() || pid < 0 {
            return Err(Errno::EINVAL.into());
        }
        let param = read_value(caller.memory(), param).await?;
        self.sched_set(caller, pid, |sched| {
            sched.legacy_attr(policy, param.sched_priority)
        })
    }

    pub(crate) async fn sys_sched_setattr(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        uattr: SchedAttrPtr,
        flags: u32,
    ) -> SysResult<i32> {
        if uattr.is_null() || pid < 0 || flags != 0 {
            return Err(Errno::EINVAL.into());
        }
        let mut attr = copy_attr(caller, uattr).await?;
        if (attr.sched_policy as i32) < 0 {
            return Err(Errno::EINVAL.into());
        }
        let keep_policy = has(attr.sched_flags, SchedFlags::SCHED_FLAG_KEEP_POLICY);
        self.sched_set(caller, pid, |sched| {
            if has(attr.sched_flags, SchedFlags::SCHED_FLAG_KEEP_PARAMS) {
                sched.params(&mut attr, None);
            }
            (attr, keep_policy)
        })
    }

    pub(crate) async fn sys_sched_getattr(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        uattr: SchedAttrPtr,
        usize: u32,
        flags: u32,
    ) -> SysResult<i32> {
        if uattr.is_null() || pid < 0 || !(ATTR_SIZE_VER0..=PAGE_SIZE as u32).contains(&usize) {
            return Err(Errno::EINVAL.into());
        }
        let now = self.host.monotonic();
        let attr = self.with_sched(caller, pid, |sched| {
            let dynamic = SchedGetattrFlags::SCHED_GETATTR_FLAG_DL_DYNAMIC.0;
            match flags {
                0 => Ok(sched.attr(None)),
                _ if sched.dl() && flags == dynamic => Ok(sched.attr(Some(now))),
                _ => Err(Errno::EINVAL),
            }
        })??;
        let attr = SchedAttr {
            size: usize.min(ATTR_SIZE_VER1),
            ..attr
        };
        // `copy_struct_to_user`: a larger buffer gets zeroes past our struct.
        let mut bytes = vec![0; usize.max(ATTR_SIZE_VER1) as usize];
        attr.write_to(&mut bytes);
        bytes.truncate(usize as usize);
        caller.memory().write(uattr.addr(), &bytes).await?;
        Ok(0)
    }

    pub(crate) fn sys_sched_priority(&self, policy: SchedPolicy, max: bool) -> SysResult<i32> {
        let (low, high) = priority_range(policy).ok_or(Errno::EINVAL)?;
        Ok(if max { high } else { low })
    }

    pub(crate) async fn sys_sched_rr_get_interval(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        interval: KernelTimespecPtr,
    ) -> SysResult<i32> {
        if pid < 0 {
            return Err(Errno::EINVAL.into());
        }
        let (policy, slice) = self.with_sched(caller, pid, |s| (s.policy, s.rr_interval()))?;
        // `sched_rr_timeslice`, which `kernel.sched_rr_timeslice_ms` sets in jiffies.
        let slice = match policy {
            SchedPolicy::SCHED_RR => {
                let ms = self
                    .tunables
                    .get(crate::sysctl::Tunable::SchedRrTimesliceMs);
                Duration::from_millis((ms as u64).div_ceil(10) * 10)
            }
            _ => slice,
        };
        // Linux never maps page 0; wasm memory starts there.
        if interval.is_null() {
            return Err(Errno::EFAULT.into());
        }
        write_value(caller.memory(), interval, &time::to_timespec(slice)).await?;
        Ok(0)
    }

    pub(crate) async fn sys_sched_getaffinity(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        len: u32,
        mask: UlongArray,
    ) -> SysResult<i32> {
        if u64::from(len) * 8 < u64::from(cpus()) || !len.is_multiple_of(MASK_WORD) {
            return Err(Errno::EINVAL.into());
        }
        let affinity = self.with_sched(caller, pid, |sched| sched.affinity.clone())?;
        if mask.is_null() {
            return Err(Errno::EFAULT.into());
        }
        let size = len.min(mask_size());
        let bytes: Vec<u8> = affinity.iter().flat_map(|w| w.to_le_bytes()).collect();
        caller
            .memory()
            .write(mask.addr(), &bytes[..size as usize])
            .await?;
        Ok(size as i32)
    }

    /// The mask is limited to the online CPUs. A deadline task must keep all of them.
    pub(crate) async fn sys_sched_setaffinity(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        len: u32,
        mask: UlongArray,
    ) -> SysResult<i32> {
        let size = len.min(mask_size()) as usize;
        let mut bytes = vec![0; mask_size() as usize];
        caller
            .memory()
            .read(mask.addr(), &mut bytes[..size])
            .await?;
        // `sched_setaffinity`: the caller's euid is the target's, or it has `CAP_SYS_NICE`
        // over the target's user namespace.
        let cred = self.cred(caller);
        let allowed = {
            let state = self.state.read_or_recover();
            let id = self.find_task(&state, caller, pid)?;
            let target = state.tasks[&id].process.cred();
            cred.same_owner(&target) || cred.ns_capable(&target.user_ns, Cap::SysNice)
        };
        if !allowed {
            return Err(Errno::EPERM.into());
        }
        let online = online_mask();
        let new: Vec<u32> = bytes
            .chunks_exact(4)
            .zip(&online)
            .map(|(word, online)| u32::from_le_bytes(word.try_into().unwrap()) & online)
            .collect();
        self.with_sched(caller, pid, |sched| {
            if sched.dl() && new != online {
                return Err(Errno::EBUSY);
            }
            if new.iter().all(|&word| word == 0) {
                return Err(Errno::EINVAL);
            }
            sched.affinity = new;
            Ok(0)
        })?
        .map_err(Into::into)
    }

    /// The task runs on the first CPU it may use.
    pub(crate) async fn sys_getcpu(
        &self,
        caller: &Caller<'_>,
        cpup: U32Ptr,
        nodep: U32Ptr,
    ) -> SysResult<i32> {
        let cpu = self.with_sched(caller, 0, |sched| {
            sched
                .affinity
                .iter()
                .enumerate()
                .find(|(_, word)| **word != 0)
                .map_or(0, |(i, word)| i as u32 * 32 + word.trailing_zeros())
        })?;
        if !cpup.is_null() {
            write_value(caller.memory(), cpup, &cpu).await?;
        }
        if !nodep.is_null() {
            write_value(caller.memory(), nodep, &0u32).await?;
        }
        Ok(0)
    }

    /// The tasks `which` and `who` name for `getpriority` and `setpriority`, each one thread:
    /// `PRIO_PROCESS` takes a thread id, and `PRIO_PGRP` and `PRIO_USER` cover every thread of
    /// their processes.
    fn priority_targets(
        &self,
        state: &State,
        caller: &Caller<'_>,
        which: PrioWhich,
        who: i32,
    ) -> Result<Vec<TaskId>, Errno> {
        let alive = |id: &TaskId| self.nameable(state, *id);
        Ok(match which {
            PrioWhich::PRIO_PROCESS => self.find_task(state, caller, who).into_iter().collect(),
            PrioWhich::PRIO_PGRP => {
                let me = &state.tasks[&caller.task()].process;
                // A group outside the caller's pid namespace has no members for it.
                let pgrp = match who {
                    0 => me.ident.lock_or_recover().pgid,
                    who => caller_pid_ns(state, caller).find(who).unwrap_or(0),
                };
                state
                    .tasks
                    .iter()
                    .filter(|(_, task)| task.process.ident.lock_or_recover().pgid == pgrp)
                    .map(|(&id, _)| id)
                    .filter(alive)
                    .collect()
            }
            PrioWhich::PRIO_USER => {
                // A uid the caller's user namespace does not map has no tasks.
                let cred = state.tasks[&caller.task()].process.cred();
                let uid = match who {
                    0 => Some(cred.user.real),
                    who => cred.user_ns.make_kuid(who as u32),
                };
                // Only the tasks the caller sees (`task_pid_vnr(p)`).
                let ns = caller_pid_ns(state, caller);
                state
                    .tasks
                    .iter()
                    .filter(|(_, task)| Some(task.process.cred().user.real) == uid)
                    .map(|(&id, _)| id)
                    .filter(|id| self.pids.nr(ns, id.0) != 0)
                    .filter(alive)
                    .collect()
            }
            _ => return Err(Errno::EINVAL),
        })
    }

    /// Returns `20 - nice` of the highest-priority target, as the raw syscall does, so it is
    /// never negative.
    pub(crate) fn sys_getpriority(
        &self,
        caller: &Caller<'_>,
        which: PrioWhich,
        who: i32,
    ) -> SysResult<i32> {
        let state = self.state.lock_or_recover();
        let targets = self.priority_targets(&state, caller, which, who)?;
        let best = targets
            .iter()
            .map(|id| 20 - state.tasks[id].sched.nice)
            .max();
        Ok(best.ok_or(Errno::ESRCH)?)
    }

    /// Sets every target's nice value (`set_one_prio`). Another user's task needs
    /// `CAP_SYS_NICE` (`EPERM`), and lowering the value needs `RLIMIT_NICE` to allow it or
    /// `CAP_SYS_NICE` (`EACCES`); a target that refuses makes the call fail, but the others
    /// still change.
    pub(crate) fn sys_setpriority(
        &self,
        caller: &Caller<'_>,
        which: PrioWhich,
        who: i32,
        nice: i32,
    ) -> SysResult<i32> {
        let nice = nice.clamp(MIN_NICE, MAX_NICE);
        let cred = self.cred(caller);
        // `can_nice`: `capable(CAP_SYS_NICE)`.
        let can_nice = cred.capable_init(Cap::SysNice);
        let mut state = self.state.lock_or_recover();
        let targets = self.priority_targets(&state, caller, which, who)?;
        let mut error = Some(Errno::ESRCH);
        for id in targets {
            let task = state.tasks.get_mut(&id).expect("listed above");
            // `set_one_prio_perm`: `CAP_SYS_NICE` over the target's user namespace.
            let target = task.process.cred();
            if !cred.same_owner(&target) && !cred.ns_capable(&target.user_ns, Cap::SysNice) {
                error = Some(Errno::EPERM);
                continue;
            }
            let limit = task
                .process
                .resources
                .lock_or_recover()
                .soft(RlimitResource::RLIMIT_NICE);
            if nice < task.sched.nice && !nice_allowed(nice, limit) && !can_nice {
                error = Some(Errno::EACCES);
                continue;
            }
            if error == Some(Errno::ESRCH) {
                error = None;
            }
            task.sched.nice = nice;
            task.css.set_nice(nice > 0);
        }
        match error {
            Some(errno) => Err(errno.into()),
            None => Ok(0),
        }
    }
}

/// `sched_copy_attr`: a size-versioned `struct sched_attr`. A size of 0 is the first version; a
/// size the kernel cannot take is `E2BIG`, with the size it wants written back.
async fn copy_attr(caller: &Caller<'_>, uattr: SchedAttrPtr) -> SysResult<SchedAttr> {
    let memory = caller.memory();
    let size = read_value(memory, wasixv2_abi::GuestPtr::<u32>::new(uattr.addr())).await?;
    let size = match size {
        0 => ATTR_SIZE_VER0,
        size => size,
    };
    let e2big = || async {
        let size = wasixv2_abi::GuestPtrMut::<u32>::new(uattr.addr());
        let _ = write_value(memory, size, &ATTR_SIZE_VER1).await;
        Errno::E2BIG
    };
    if !(ATTR_SIZE_VER0..=PAGE_SIZE as u32).contains(&size) {
        return Err(e2big().await.into());
    }
    // `copy_struct_from_user`: bytes past what we know must be zero.
    let mut bytes = vec![0; size.max(ATTR_SIZE_VER1) as usize];
    memory
        .read(uattr.addr(), &mut bytes[..size as usize])
        .await?;
    if bytes[ATTR_SIZE_VER1 as usize..].iter().any(|&b| b != 0) {
        return Err(e2big().await.into());
    }
    let mut attr = SchedAttr::read_from(&bytes[..ATTR_SIZE_VER1 as usize]).ok_or(Errno::EFAULT)?;
    if has(attr.sched_flags, SchedFlags::SCHED_FLAG_UTIL_CLAMP) && size < ATTR_SIZE_VER1 {
        return Err(Errno::EINVAL.into());
    }
    attr.sched_nice = attr.sched_nice.clamp(MIN_NICE, MAX_NICE);
    Ok(attr)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn admission() -> Admission {
        Admission {
            nice_limit: 0,
            rtprio_limit: 0,
            capable: false,
            same_owner: true,
            dl_others: 0,
            now: Duration::from_secs(1),
        }
    }

    fn privileged() -> Admission {
        Admission {
            capable: true,
            ..admission()
        }
    }

    fn policy(policy: SchedPolicy, priority: u32) -> SchedAttr {
        SchedAttr {
            sched_policy: policy.0 as u32,
            sched_priority: priority,
            ..SchedAttr::default()
        }
    }

    fn deadline(runtime: u64, deadline: u64, period: u64) -> SchedAttr {
        SchedAttr {
            sched_policy: SchedPolicy::SCHED_DEADLINE.0 as u32,
            sched_runtime: runtime,
            sched_deadline: deadline,
            sched_period: period,
            ..SchedAttr::default()
        }
    }

    #[test]
    fn realtime_needs_rtprio_or_cap_sys_nice() {
        let mut sched = Sched::default();
        let fifo = policy(SchedPolicy::SCHED_FIFO, 10);
        assert_eq!(sched.set(&fifo, false, &admission()), Err(Errno::EPERM));
        let limited = Admission {
            rtprio_limit: 5,
            ..admission()
        };
        assert_eq!(sched.set(&fifo, false, &limited), Err(Errno::EPERM));
        let low = policy(SchedPolicy::SCHED_FIFO, 5);
        assert_eq!(sched.set(&low, false, &limited), Ok(()));
        assert_eq!(sched.set(&fifo, false, &privileged()), Ok(()));
        assert_eq!((sched.policy_word(), sched.param().sched_priority), (1, 10));
        // Lowering the priority is always allowed, raising it again is not.
        assert_eq!(sched.set(&low, false, &admission()), Ok(()));
        assert_eq!(sched.set(&fifo, false, &admission()), Err(Errno::EPERM));
        // Real-time tasks have no timer slack and are boosted.
        assert_eq!(sched.timer_slack(), 0);
        assert_eq!(sched.attr(None).sched_util_min, UCLAMP_MIN_RT_DEFAULT);
        assert_eq!(
            sched.set(&policy(SchedPolicy::SCHED_NORMAL, 0), false, &admission()),
            Ok(())
        );
        assert_eq!(sched.timer_slack(), TIMER_SLACK_NS);
        assert_eq!(sched.attr(None).sched_util_min, 0);
    }

    #[test]
    fn priorities_must_match_the_policy() {
        let mut sched = Sched::default();
        let adm = privileged();
        for (attr, errno) in [
            (policy(SchedPolicy::SCHED_FIFO, 0), Errno::EINVAL),
            (policy(SchedPolicy::SCHED_RR, 100), Errno::EINVAL),
            (policy(SchedPolicy::SCHED_NORMAL, 1), Errno::EINVAL),
            (policy(SchedPolicy::SCHED_EXT, 0), Errno::EINVAL),
            (policy(SchedPolicy(42), 0), Errno::EINVAL),
        ] {
            assert_eq!(sched.set(&attr, false, &adm), Err(errno));
        }
        // sched_setparam keeps the policy: a priority is wrong for SCHED_OTHER.
        let (attr, keep) = sched.legacy_attr(None, 3);
        assert_eq!(sched.set(&attr, keep, &adm), Err(Errno::EINVAL));
        assert_eq!(sched, Sched::default());
    }

    #[test]
    fn nice_and_idle_follow_rlimit_nice() {
        let mut sched = Sched::default();
        let nicer = SchedAttr {
            sched_nice: 5,
            ..policy(SchedPolicy::SCHED_BATCH, 0)
        };
        assert_eq!(sched.set(&nicer, false, &admission()), Ok(()));
        assert_eq!((sched.policy, sched.nice), (SchedPolicy::SCHED_BATCH, 5));
        let back = policy(SchedPolicy::SCHED_NORMAL, 0);
        assert_eq!(sched.set(&back, false, &admission()), Err(Errno::EPERM));
        let limit = Admission {
            nice_limit: 20,
            ..admission()
        };
        assert_eq!(sched.set(&back, false, &limit), Ok(()));
        // Leaving SCHED_IDLE counts as raising the priority from nice 20.
        let idle = policy(SchedPolicy::SCHED_IDLE, 0);
        assert_eq!(sched.set(&idle, false, &admission()), Ok(()));
        let keep_nice = SchedAttr {
            sched_nice: 0,
            ..back
        };
        assert_eq!(
            sched.set(&keep_nice, false, &admission()),
            Err(Errno::EPERM)
        );
        assert_eq!(sched.set(&keep_nice, false, &limit), Ok(()));
    }

    #[test]
    fn reset_on_fork_is_sticky_and_applies_at_fork() {
        let mut sched = Sched::default();
        let (attr, keep) =
            sched.legacy_attr(Some(SchedPolicy(SchedPolicy::SCHED_RR.0 | 0x4000_0000)), 7);
        assert_eq!(sched.set(&attr, keep, &privileged()), Ok(()));
        assert_eq!(sched.policy_word(), SchedPolicy::SCHED_RR.0 | 0x4000_0000);
        // Clearing the flag needs privilege; sched_setparam keeps it.
        let (attr, keep) = sched.legacy_attr(Some(SchedPolicy::SCHED_RR), 7);
        assert_eq!(sched.set(&attr, keep, &admission()), Err(Errno::EPERM));
        let (attr, keep) = sched.legacy_attr(None, 3);
        assert_eq!(sched.set(&attr, keep, &admission()), Ok(()));
        assert_eq!(sched.policy_word(), SchedPolicy::SCHED_RR.0 | 0x4000_0000);
        let child = sched.for_child().unwrap();
        assert_eq!((child.policy_word(), child.param().sched_priority), (0, 0));
        assert_eq!(child.timer_slack(), 0, "the parent's slack at fork");
        // Without the flag, the child inherits everything.
        let mut plain = Sched::default();
        let attr = policy(SchedPolicy::SCHED_FIFO, 20);
        assert_eq!(plain.set(&attr, false, &privileged()), Ok(()));
        let child = plain.for_child().unwrap();
        assert_eq!((child.policy_word(), child.param().sched_priority), (1, 20));
    }

    #[test]
    fn deadline_is_validated_admitted_and_cannot_fork() {
        let mut sched = Sched::default();
        let ms = 1_000_000;
        assert_eq!(
            sched.set(&deadline(10 * ms, 30 * ms, 100 * ms), false, &admission()),
            Err(Errno::EPERM)
        );
        for bad in [
            deadline(10 * ms, 0, 0),
            deadline(1000, 30 * ms, 100 * ms),
            deadline(40 * ms, 30 * ms, 100 * ms),
            deadline(10 * ms, 30 * ms, 20 * ms),
            deadline(10_000, 50_000, 50_000),
            deadline(10 * ms, 30 * ms, 5_000_000 * ms),
        ] {
            assert_eq!(sched.set(&bad, false, &privileged()), Err(Errno::EINVAL));
        }
        let full = Admission {
            dl_others: dl_capacity(),
            ..privileged()
        };
        let attr = deadline(10 * ms, 30 * ms, 100 * ms);
        assert_eq!(sched.set(&attr, false, &full), Err(Errno::EBUSY));
        assert_eq!(sched.set(&attr, false, &privileged()), Ok(()));
        let got = sched.attr(None);
        assert_eq!(
            (
                got.sched_policy,
                got.sched_runtime,
                got.sched_deadline,
                got.sched_period
            ),
            (6, 10 * ms, 30 * ms, 100 * ms)
        );
        assert_eq!(sched.dl_held(), to_ratio(100 * ms, 10 * ms));
        // The current period's absolute deadline.
        let now = Duration::from_secs(1) + Duration::from_millis(250);
        let dynamic = sched.attr(Some(now));
        assert_eq!(dynamic.sched_deadline, 1_200_000_000 + 30 * ms);
        assert_eq!(sched.for_child(), Err(Errno::EAGAIN));
        assert_eq!(sched.rr_interval(), Duration::ZERO);
    }

    #[test]
    fn utilization_clamps_are_validated_and_remembered() {
        let mut sched = Sched::default();
        let clamp = |flags: SchedFlags, min: u32, max: u32| SchedAttr {
            sched_flags: flags,
            sched_util_min: min,
            sched_util_max: max,
            ..policy(SchedPolicy::SCHED_NORMAL, 0)
        };
        let both = SchedFlags::SCHED_FLAG_UTIL_CLAMP;
        assert_eq!(
            sched.set(&clamp(both, 1025, 1024), false, &admission()),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            sched.set(&clamp(both, 600, 500), false, &admission()),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            sched.set(&clamp(both, 100, 500), false, &admission()),
            Ok(())
        );
        let got = sched.attr(None);
        assert_eq!((got.sched_util_min, got.sched_util_max), (100, 500));
        // -1 restores the default.
        let min = SchedFlags::SCHED_FLAG_UTIL_CLAMP_MIN;
        assert_eq!(
            sched.set(&clamp(min, u32::MAX, 0), false, &admission()),
            Ok(())
        );
        let got = sched.attr(None);
        assert_eq!((got.sched_util_min, got.sched_util_max), (0, 500));
    }

    #[test]
    fn fair_slices_and_timeslices() {
        let mut sched = Sched::default();
        assert_eq!(sched.rr_interval(), in_ticks(base_slice()));
        let custom = SchedAttr {
            sched_runtime: 5_500_000,
            ..policy(SchedPolicy::SCHED_NORMAL, 0)
        };
        assert_eq!(sched.set(&custom, false, &admission()), Ok(()));
        assert_eq!(sched.attr(None).sched_runtime, 5_500_000);
        assert_eq!(sched.rr_interval(), Duration::from_millis(5));
        let rr = policy(SchedPolicy::SCHED_RR, 1);
        assert_eq!(sched.set(&rr, false, &privileged()), Ok(()));
        assert_eq!(sched.rr_interval(), RR_TIMESLICE);
    }

    #[test]
    fn timer_slack_defaults_and_rt_tasks_have_none() {
        let mut sched = Sched::default();
        sched.set_timer_slack(1234);
        assert_eq!(sched.timer_slack(), 1234);
        let mut child = sched.for_child().unwrap();
        child.set_timer_slack(99);
        child.set_timer_slack(0);
        assert_eq!(
            child.timer_slack(),
            1234,
            "the default is the parent's slack"
        );
        let fifo = policy(SchedPolicy::SCHED_FIFO, 1);
        assert_eq!(child.set(&fifo, false, &privileged()), Ok(()));
        child.set_timer_slack(5);
        assert_eq!(child.timer_slack(), 0);
    }
}
