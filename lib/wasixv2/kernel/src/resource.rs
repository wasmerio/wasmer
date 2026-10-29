//! Resources and accounting: resource limits, usage and times, `sysinfo`, entropy and
//! `membarrier`.

use std::{sync::atomic::Ordering, time::Duration};

use wasixv2_abi::{
    ConstRlimit64Ptr, GrndFlags, KernelClock, KernelOldTimeval, MembarrierCmd, MembarrierCmdFlags,
    Pid, Rlimit64, Rlimit64Ptr, RlimitResource, Rusage, RusagePtr, RusageWho, Sysinfo, SysinfoPtr,
    Tms, TmsPtr,
};
use wasixv2_syscalls::{Caller, CpuTimes, Errno, SysResult, UserSliceMut, read_value, write_value};

use wasixv2_vfs::Cap;

use crate::{Kernel, Process, State, file::NrOpen, mm::PAGE_SIZE, sync::MutexExt, uidgid::Kuid};

const INFINITY: u64 = u64::MAX;
/// `RLIM_NLIMITS`.
const NLIMITS: usize = 16;
/// Clock ticks per second (`USER_HZ`).
pub(crate) const TICKS: u128 = 100;
/// The address space a wasm32 guest can grow into.
pub(crate) const ADDRESS_SPACE: u64 = 1 << 32;
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

/// Per-process limits and `membarrier` registration. Scheduling state is per task, in `sched`.
#[derive(Clone)]
pub(crate) struct Resources {
    limits: [Rlimit64; NLIMITS],
    membarrier_private: bool,
}

impl Default for Resources {
    fn default() -> Self {
        Self {
            limits: DEFAULT_LIMITS.map(|(rlim_cur, rlim_max)| Rlimit64 { rlim_cur, rlim_max }),
            membarrier_private: false,
        }
    }
}

/// `copy_process`'s `RLIMIT_NPROC` check for a new task of `process`: a user other than root
/// without `CAP_SYS_RESOURCE` or `CAP_SYS_ADMIN` cannot have more tasks than the soft limit.
pub(crate) fn nproc_exceeded(state: &State, process: &Process) -> bool {
    let cred = process.cred();
    let uid = cred.user.real;
    if uid == Kuid::GLOBAL_ROOT || cred.capable(Cap::SysResource) || cred.capable(Cap::SysAdmin) {
        return false;
    }
    let tasks = state
        .tasks
        .values()
        .filter(|task| task.process.cred().user.real == uid)
        .count();
    process.resources.lock_or_recover().nproc_reached(tasks)
}

impl Resources {
    /// Every limit, indexed by `RLIMIT_*`.
    pub(crate) fn limits(&self) -> &[Rlimit64; NLIMITS] {
        &self.limits
    }

    /// Whether `tasks`, how many the user has, reach the soft `RLIMIT_NPROC`.
    fn nproc_reached(&self, tasks: usize) -> bool {
        let limit = self.limits[RlimitResource::RLIMIT_NPROC.0 as usize].rlim_cur;
        tasks as u64 >= limit
    }

    /// The soft limit of `resource`.
    pub(crate) fn soft(&self, resource: RlimitResource) -> u64 {
        self.limits[resource.0 as usize].rlim_cur
    }

    /// Raises or lowers the soft limit of `resource`, as the kernel itself does to
    /// `RLIMIT_CPU`'s once it sent SIGXCPU.
    pub(crate) fn set_soft(&mut self, resource: RlimitResource, soft: u64) {
        self.limits[resource.0 as usize].rlim_cur = soft;
    }

    pub(crate) fn nofile(&self) -> usize {
        self.limits[RlimitResource::RLIMIT_NOFILE.0 as usize]
            .rlim_cur
            .min(u64::from(NrOpen::MAX)) as usize
    }

    pub(crate) fn sigpending(&self) -> u64 {
        self.limits[RlimitResource::RLIMIT_SIGPENDING.0 as usize].rlim_cur
    }
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

/// A `struct rusage` with `times` and `maxrss` (in KiB); nothing else is counted.
fn rusage(times: CpuTimes, maxrss: u64) -> Rusage {
    Rusage {
        ru_utime: timeval(times.user),
        ru_stime: timeval(times.system),
        ru_maxrss: maxrss.min(i32::MAX as u64) as i32,
        ..Rusage::default()
    }
}

impl Process {
    /// `getrusage(p, RUSAGE_BOTH)`, which `waitid` reports of the child it waited for: its own
    /// time with its reaped children's, and the larger of their `maxrss`.
    pub(crate) fn usage_both(&self) -> Rusage {
        let children = *self.children.lock_or_recover();
        let maxrss = self.maxrss.load(Ordering::Relaxed).max(children.maxrss);
        rusage(self.cpu.times().saturating_add(children.times), maxrss)
    }

    /// `wait_task_zombie` reaped `child`: its time and its reaped children's go to the
    /// caller's `cutime` and `cstime`, and the larger `maxrss` of theirs to `cmaxrss`.
    pub(crate) fn add_child_usage(&self, child: &Process) {
        let theirs = *child.children.lock_or_recover();
        let maxrss = child.maxrss.load(Ordering::Relaxed).max(theirs.maxrss);
        let mut children = self.children.lock_or_recover();
        children.times = children
            .times
            .saturating_add(child.cpu.times())
            .saturating_add(theirs.times);
        children.maxrss = children.maxrss.max(maxrss);
    }

    /// Records that one of its tasks left a memory of `size` bytes (`setmax_mm_hiwater_rss`
    /// in `do_exit`).
    pub(crate) fn note_rss(&self, size: u64) {
        self.maxrss.fetch_max(size / 1024, Ordering::Relaxed);
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
        let process = self.lookup_pid(caller, pid)?;
        let index = usize::try_from(resource.0)
            .ok()
            .filter(|&i| i < NLIMITS)
            .ok_or(Errno::EINVAL)?;
        let cred = self.cred(caller);
        if !std::ptr::eq(&*process, &*self.process(caller.task()))
            && !cred.may_change_limits_of(&process.cred())
        {
            return Err(Errno::EPERM.into());
        }
        let previous = {
            let mut resources = process.resources.lock_or_recover();
            let previous = resources.limits[index];
            if let Some(new) = new {
                if new.rlim_cur > new.rlim_max {
                    return Err(Errno::EINVAL.into());
                }
                let nofile = resource == RlimitResource::RLIMIT_NOFILE;
                let raises = new.rlim_max > previous.rlim_max && !cred.capable(Cap::SysResource);
                // `fs.nr_open` bounds the hard limit, even for a caller that may raise it.
                if raises || (nofile && new.rlim_max > u64::from(self.nr_open.get())) {
                    return Err(Errno::EPERM.into());
                }
                resources.limits[index] = new;
            }
            previous
        };
        // `update_rlimit_cpu`: the soft limit is an expiry of the process's CPU clock.
        if new.is_some() && resource == RlimitResource::RLIMIT_CPU {
            process.rlimit_cpu_changed();
        }
        if new.is_some() && resource == RlimitResource::RLIMIT_NOFILE {
            // The limit is the target's, not the caller's: every descriptor table its tasks use.
            let limit = process.resources.lock_or_recover().nofile();
            let state = self.state.read_or_recover();
            let tasks = state.tasks.values();
            for task in tasks.filter(|task| std::sync::Arc::ptr_eq(&task.process, &process)) {
                task.files.lock_or_recover().set_limit(limit);
            }
        }
        if !old.is_null() {
            write_value(caller.memory(), old, &previous).await?;
        }
        Ok(0)
    }

    /// `getrusage`: the CPU time of the caller's thread group, of the caller alone, or of the
    /// group's reaped children, as charged so far; `maxrss` is the most memory the group's
    /// tasks used, which is the memory they share now unless one left more.
    pub(crate) async fn sys_getrusage(
        &self,
        caller: &Caller<'_>,
        who: RusageWho,
        ru: RusagePtr,
    ) -> SysResult<i32> {
        let process = self.process(caller.task());
        let usage = match who {
            RusageWho::RUSAGE_SELF | RusageWho::RUSAGE_THREAD => {
                let times = match who {
                    RusageWho::RUSAGE_SELF => process.cpu.times(),
                    _ => self.task_ref(caller.task(), |t| t.cpu.times()),
                };
                let size = caller.memory().size().await / 1024;
                rusage(times, process.maxrss.load(Ordering::Relaxed).max(size))
            }
            RusageWho::RUSAGE_CHILDREN => {
                let children = *process.children.lock_or_recover();
                rusage(children.times, children.maxrss)
            }
            _ => return Err(Errno::EINVAL.into()),
        };
        write_value(caller.memory(), ru, &usage).await?;
        Ok(0)
    }

    /// `do_sys_times`: the CPU time of the caller's thread group and of its reaped children.
    pub(crate) async fn sys_times(&self, caller: &Caller<'_>, tbuf: TmsPtr) -> SysResult<i32> {
        if !tbuf.is_null() {
            let process = self.process(caller.task());
            let (own, children) = (
                process.cpu.times(),
                process.children.lock_or_recover().times,
            );
            let tms = Tms {
                tms_utime: ticks(own.user),
                tms_stime: ticks(own.system),
                tms_cutime: ticks(children.user),
                tms_cstime: ticks(children.system),
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
        let procs = self.state.read_or_recover().tasks.len();
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
        let mut resources = process.resources.lock_or_recover();
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
