//! Resources and accounting: resource limits, usage and times, `sysinfo`, entropy, and the
//! scheduler interface (policies, nice values, affinity), plus `membarrier`.

use std::{future::poll_fn, task::Poll, time::Duration};

use wasixv2_abi::{
    ConstRlimit64Ptr, ConstSchedParamPtr, GrndFlags, KernelClock, KernelOldTimeval,
    KernelTimespecPtr, MembarrierCmd, MembarrierCmdFlags, Pid, PrioWhich, Rlimit64, Rlimit64Ptr,
    RlimitResource, Rusage, RusagePtr, RusageWho, SchedParam, SchedParamPtr, SchedPolicy, Sysinfo,
    SysinfoPtr, Tms, TmsPtr, U32Ptr, UlongArray,
};
use wasixv2_syscalls::{Caller, Errno, SysResult, UserSliceMut, read_value, write_value};

use crate::{ID, Kernel, mm::PAGE_SIZE, time};

const INFINITY: u64 = u64::MAX;
/// `RLIM_NLIMITS`.
const NLIMITS: usize = 16;
/// `fs.nr_open`: the most `RLIMIT_NOFILE` can be raised to.
const NR_OPEN: u64 = 1 << 20;
/// Clock ticks per second (`USER_HZ`).
pub(crate) const TICKS: u128 = 100;
/// The address space a wasm32 guest can grow into.
pub(crate) const ADDRESS_SPACE: u64 = 1 << 32;
/// Bytes per guest `cpu_set_t` word (`unsigned long`).
const MASK_WORD: u32 = 4;
const GETRANDOM_CHUNK: usize = 64 * 1024;

/// Linux's defaults for an unprivileged process.
const DEFAULT_LIMITS: [(u64, u64); NLIMITS] = [
    (INFINITY, INFINITY), // CPU
    (INFINITY, INFINITY), // FSIZE
    (INFINITY, INFINITY), // DATA
    (8 << 20, INFINITY),  // STACK
    (0, INFINITY),        // CORE
    (INFINITY, INFINITY), // RSS
    (4096, 4096),         // NPROC
    (1024, 4096),         // NOFILE
    (8 << 20, 8 << 20),   // MEMLOCK
    (INFINITY, INFINITY), // AS
    (INFINITY, INFINITY), // LOCKS
    (4096, 4096),         // SIGPENDING
    (819_200, 819_200),   // MSGQUEUE
    (0, 0),               // NICE
    (0, 0),               // RTPRIO
    (INFINITY, INFINITY), // RTTIME
];

/// Per-process limits and scheduling state.
// TODO: nice, policy and affinity are per thread on Linux.
#[derive(Clone)]
pub(crate) struct Resources {
    limits: [Rlimit64; NLIMITS],
    nice: i32,
    policy: SchedPolicy,
    /// CPUs the process may run on, one bit each.
    affinity: Vec<u32>,
    membarrier_private: bool,
}

impl Default for Resources {
    fn default() -> Self {
        Self {
            limits: DEFAULT_LIMITS.map(|(rlim_cur, rlim_max)| Rlimit64 { rlim_cur, rlim_max }),
            nice: 0,
            policy: SchedPolicy::SCHED_NORMAL,
            affinity: full_mask(cpus()),
            membarrier_private: false,
        }
    }
}

impl Resources {
    /// Every limit, indexed by `RLIMIT_*`.
    pub(crate) fn limits(&self) -> &[Rlimit64; NLIMITS] {
        &self.limits
    }

    pub(crate) fn nice(&self) -> i32 {
        self.nice
    }

    /// `copy_process`'s `RLIMIT_NPROC` check, given how many tasks the user has: a user other
    /// than root without `CAP_SYS_RESOURCE` or `CAP_SYS_ADMIN`, which every process here is,
    /// cannot have more tasks than the soft limit.
    pub(crate) fn nproc_reached(&self, tasks: usize) -> bool {
        let limit = self.limits[RlimitResource::RLIMIT_NPROC.0 as usize].rlim_cur;
        tasks as u64 >= limit
    }

    pub(crate) fn nofile(&self) -> usize {
        self.limits[RlimitResource::RLIMIT_NOFILE.0 as usize]
            .rlim_cur
            .min(NR_OPEN) as usize
    }
}

/// CPUs the host offers.
pub(crate) fn cpus() -> u32 {
    std::thread::available_parallelism().map_or(1, |n| n.get().min(1024) as u32)
}

fn full_mask(cpus: u32) -> Vec<u32> {
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

fn timeval(d: Duration) -> KernelOldTimeval {
    KernelOldTimeval {
        tv_sec: d.as_secs() as i32,
        tv_usec: d.subsec_micros() as i32,
    }
}

fn ticks(d: Duration) -> KernelClock {
    (d.as_millis() * TICKS / 1000) as KernelClock & i32::MAX
}

/// Priority range of `policy`, or `None` if it is unknown.
fn priority_range(policy: SchedPolicy) -> Option<(i32, i32)> {
    match policy {
        SchedPolicy::SCHED_FIFO | SchedPolicy::SCHED_RR => Some((1, 99)),
        SchedPolicy::SCHED_NORMAL
        | SchedPolicy::SCHED_BATCH
        | SchedPolicy::SCHED_IDLE
        | SchedPolicy::SCHED_DEADLINE => Some((0, 0)),
        _ => None,
    }
}

impl Kernel {
    pub(crate) async fn sys_prlimit64(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        resource: RlimitResource,
        new: ConstRlimit64Ptr,
        old: Rlimit64Ptr,
    ) -> SysResult<i32> {
        let new = match new.is_null() {
            true => None,
            false => Some(read_value(caller.memory(), new).await?),
        };
        let process = self.lookup_pid(caller, pid).map_err(|_| Errno::ESRCH)?;
        let index = usize::try_from(resource.0)
            .ok()
            .filter(|&i| i < NLIMITS)
            .ok_or(Errno::EINVAL)?;
        let previous = {
            let mut resources = process.resources.lock().unwrap();
            let previous = resources.limits[index];
            if let Some(new) = new {
                if new.rlim_cur > new.rlim_max {
                    return Err(Errno::EINVAL.into());
                }
                let nofile = resource == RlimitResource::RLIMIT_NOFILE;
                if new.rlim_max > previous.rlim_max || (nofile && new.rlim_max > NR_OPEN) {
                    return Err(Errno::EPERM.into());
                }
                resources.limits[index] = new;
            }
            previous
        };
        if new.is_some() && resource == RlimitResource::RLIMIT_NOFILE {
            // The limit is the target's, not the caller's: every descriptor table its tasks use.
            let limit = process.resources.lock().unwrap().nofile();
            let state = self.state.read().unwrap();
            let tasks = state.tasks.values();
            for task in tasks.filter(|task| std::sync::Arc::ptr_eq(&task.process, &process)) {
                task.files.lock().unwrap().set_limit(limit);
            }
        }
        if !old.is_null() {
            write_value(caller.memory(), old, &previous).await?;
        }
        Ok(0)
    }

    /// The guest's CPU time so far; see `time::Clock::ProcessCpu`.
    fn cpu_time(&self, caller: &Caller<'_>) -> Duration {
        self.clock_now(caller, time::Clock::ProcessCpu)
    }

    pub(crate) async fn sys_getrusage(
        &self,
        caller: &Caller<'_>,
        who: RusageWho,
        ru: RusagePtr,
    ) -> SysResult<i32> {
        let mut usage = Rusage::default();
        match who {
            RusageWho::RUSAGE_SELF | RusageWho::RUSAGE_THREAD => {
                usage.ru_utime = timeval(self.cpu_time(caller));
                usage.ru_maxrss = (caller.memory().size().await / 1024) as i32;
            }
            // No process has children yet.
            RusageWho::RUSAGE_CHILDREN => {}
            _ => return Err(Errno::EINVAL.into()),
        }
        write_value(caller.memory(), ru, &usage).await?;
        Ok(0)
    }

    pub(crate) async fn sys_times(&self, caller: &Caller<'_>, tbuf: TmsPtr) -> SysResult<i32> {
        if !tbuf.is_null() {
            let tms = Tms {
                tms_utime: ticks(self.cpu_time(caller)),
                ..Tms::default()
            };
            write_value(caller.memory(), tbuf, &tms).await?;
        }
        Ok(ticks(self.host.monotonic()))
    }

    /// Memory is counted in wasm pages of the 4 GiB a guest can address.
    pub(crate) async fn sys_sysinfo(
        &self,
        caller: &Caller<'_>,
        info: SysinfoPtr,
    ) -> SysResult<i32> {
        if info.is_null() {
            return Err(Errno::EFAULT.into());
        }
        let uptime = self.host.monotonic();
        let total = ADDRESS_SPACE / PAGE_SIZE;
        let used = caller.memory().size().await / PAGE_SIZE;
        let procs = self.state.read().unwrap().tasks.len();
        let si = Sysinfo {
            uptime: (uptime.as_secs() + u64::from(uptime.subsec_nanos() > 0)) as i32,
            totalram: total as u32,
            freeram: total.saturating_sub(used).max(1) as u32,
            procs: procs as u16,
            mem_unit: PAGE_SIZE as u32,
            ..Sysinfo::default()
        };
        write_value(caller.memory(), info, &si).await?;
        Ok(0)
    }

    pub(crate) async fn sys_getrandom(
        &self,
        caller: &Caller<'_>,
        buf: UserSliceMut,
        flags: GrndFlags,
    ) -> SysResult<u32> {
        let known = GrndFlags::GRND_NONBLOCK | GrndFlags::GRND_RANDOM | GrndFlags::GRND_INSECURE;
        if !(flags & !known).is_empty()
            || flags.contains(GrndFlags::GRND_INSECURE | GrndFlags::GRND_RANDOM)
        {
            return Err(Errno::EINVAL.into());
        }
        let len = buf.len.min(i32::MAX as u32);
        if len > 0 && buf.addr == 0 {
            return Err(Errno::EFAULT.into());
        }
        let mut chunk = vec![0; GETRANDOM_CHUNK];
        let mut done = 0;
        while done < len {
            let n = ((len - done) as usize).min(GETRANDOM_CHUNK);
            self.host.random(&mut chunk[..n]);
            if caller
                .memory()
                .write(buf.addr + done, &chunk[..n])
                .await
                .is_err()
            {
                return match done {
                    0 => Err(Errno::EFAULT.into()),
                    _ => Ok(done),
                };
            }
            done += n as u32;
        }
        Ok(done)
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
        Ok(self
            .lookup_pid(caller, pid)?
            .resources
            .lock()
            .unwrap()
            .policy
            .0)
    }

    pub(crate) async fn sys_sched_getparam(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        param: SchedParamPtr,
    ) -> SysResult<i32> {
        if param.is_null() {
            return Err(Errno::EINVAL.into());
        }
        self.lookup_pid(caller, pid)?;
        write_value(caller.memory(), param, &SchedParam { sched_priority: 0 }).await?;
        Ok(0)
    }

    /// `policy` of `None` keeps the current one (`sched_setparam`). Real-time policies need a
    /// privilege no process has.
    pub(crate) async fn sys_sched_setscheduler(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        policy: Option<SchedPolicy>,
        param: ConstSchedParamPtr,
    ) -> SysResult<i32> {
        if param.is_null() || pid < 0 {
            return Err(Errno::EINVAL.into());
        }
        let param = read_value(caller.memory(), param).await?;
        let process = self.lookup_pid(caller, pid)?;
        let mut resources = process.resources.lock().unwrap();
        let policy = policy.map_or(resources.policy, |p| {
            SchedPolicy(p.0 & !SchedPolicy::SCHED_RESET_ON_FORK.0)
        });
        let (min, max) = priority_range(policy).ok_or(Errno::EINVAL)?;
        if policy == SchedPolicy::SCHED_DEADLINE || !(min..=max).contains(&param.sched_priority) {
            return Err(Errno::EINVAL.into());
        }
        if matches!(policy, SchedPolicy::SCHED_FIFO | SchedPolicy::SCHED_RR) {
            return Err(Errno::EPERM.into());
        }
        resources.policy = policy;
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
        let policy = self
            .lookup_pid(caller, pid)?
            .resources
            .lock()
            .unwrap()
            .policy;
        let slice = match policy {
            SchedPolicy::SCHED_RR => Duration::from_millis(100),
            _ => Duration::ZERO,
        };
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
        let affinity = self
            .lookup_pid(caller, pid)?
            .resources
            .lock()
            .unwrap()
            .affinity
            .clone();
        let size = len.min(mask_size());
        let bytes: Vec<u8> = affinity.iter().flat_map(|w| w.to_le_bytes()).collect();
        caller
            .memory()
            .write(mask.addr(), &bytes[..size as usize])
            .await?;
        Ok(size as i32)
    }

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
        let online = full_mask(cpus());
        let new: Vec<u32> = bytes
            .chunks_exact(4)
            .zip(&online)
            .map(|(word, online)| u32::from_le_bytes(word.try_into().unwrap()) & online)
            .collect();
        if new.iter().all(|&word| word == 0) {
            return Err(Errno::EINVAL.into());
        }
        self.lookup_pid(caller, pid)?
            .resources
            .lock()
            .unwrap()
            .affinity = new;
        Ok(0)
    }

    /// The task runs on the first CPU it may use.
    pub(crate) async fn sys_getcpu(
        &self,
        caller: &Caller<'_>,
        cpup: U32Ptr,
        nodep: U32Ptr,
    ) -> SysResult<i32> {
        let affinity = self
            .process(caller.task())
            .resources
            .lock()
            .unwrap()
            .affinity
            .clone();
        let cpu = affinity
            .iter()
            .enumerate()
            .find(|(_, word)| **word != 0)
            .map_or(0, |(i, word)| i as u32 * 32 + word.trailing_zeros());
        if !cpup.is_null() {
            write_value(caller.memory(), cpup, &cpu).await?;
        }
        if !nodep.is_null() {
            write_value(caller.memory(), nodep, &0u32).await?;
        }
        Ok(0)
    }

    /// Checks `which` and `who` and returns the process they name.
    fn priority_target(
        &self,
        caller: &Caller<'_>,
        which: PrioWhich,
        who: i32,
    ) -> Result<std::sync::Arc<crate::Process>, Errno> {
        let process = self.process(caller.task());
        let matches = match which {
            PrioWhich::PRIO_PROCESS => who == 0 || who as u32 == process.pid.0,
            PrioWhich::PRIO_PGRP => who == 0 || who as u32 == process.ident.lock().unwrap().pgid,
            PrioWhich::PRIO_USER => who == 0 || who == ID,
            _ => return Err(Errno::EINVAL),
        };
        match matches {
            true => Ok(process),
            false => Err(Errno::ESRCH),
        }
    }

    /// Returns `20 - nice`, as the raw syscall does, so it is never negative.
    pub(crate) fn sys_getpriority(
        &self,
        caller: &Caller<'_>,
        which: PrioWhich,
        who: i32,
    ) -> SysResult<i32> {
        let process = self.priority_target(caller, which, who)?;
        let nice = process.resources.lock().unwrap().nice;
        Ok(20 - nice)
    }

    /// Lowering the nice value needs `RLIMIT_NICE` to allow it.
    pub(crate) fn sys_setpriority(
        &self,
        caller: &Caller<'_>,
        which: PrioWhich,
        who: i32,
        nice: i32,
    ) -> SysResult<i32> {
        let process = self.priority_target(caller, which, who)?;
        let nice = nice.clamp(-20, 19);
        let mut resources = process.resources.lock().unwrap();
        let limit = resources.limits[RlimitResource::RLIMIT_NICE.0 as usize].rlim_cur;
        if nice < resources.nice && (20 - nice) as u64 > limit {
            return Err(Errno::EACCES.into());
        }
        resources.nice = nice;
        Ok(0)
    }

    /// Wasm memory accesses are ordered by the engine, so no barrier needs to be issued; this
    /// only keeps Linux's command and registration rules.
    pub(crate) fn sys_membarrier(
        &self,
        caller: &Caller<'_>,
        cmd: MembarrierCmd,
        flags: MembarrierCmdFlags,
    ) -> SysResult<i32> {
        const SUPPORTED: i32 = 0b1_1111;
        if !flags.is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let process = self.process(caller.task());
        let mut resources = process.resources.lock().unwrap();
        match cmd.0 {
            0 => Ok(SUPPORTED),
            1 | 2 | 4 => Ok(0),
            8 if resources.membarrier_private => Ok(0),
            8 => Err(Errno::EPERM.into()),
            16 => {
                resources.membarrier_private = true;
                Ok(0)
            }
            _ => Err(Errno::EINVAL.into()),
        }
    }
}
