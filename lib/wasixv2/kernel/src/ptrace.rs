//! Process tracing (kernel/ptrace.c): who may look into or attach to another process
//! (`ptrace_may_access`, with the capability, Yama and Landlock rules of the LSMs this kernel
//! has).

use std::sync::Arc;

use wasixv2_syscalls::TaskId;

use wasixv2_vfs::Cap;

use crate::{Kernel, Process, landlock::Domain, sync::MutexExt as _, yama};

/// `PTRACE_MODE_READ`: to read the target's state.
pub(crate) const MODE_READ: u32 = 0x01;
/// `PTRACE_MODE_ATTACH`: to attach to the target, or change it.
pub(crate) const MODE_ATTACH: u32 = 0x02;
/// `PTRACE_MODE_FSCREDS`: the caller's filesystem ids and effective capabilities count.
pub(crate) const MODE_FSCREDS: u32 = 0x08;
/// `PTRACE_MODE_REALCREDS`: the caller's real ids and permitted capabilities count.
pub(crate) const MODE_REALCREDS: u32 = 0x10;

pub(crate) const MODE_READ_FSCREDS: u32 = MODE_READ | MODE_FSCREDS;
pub(crate) const MODE_READ_REALCREDS: u32 = MODE_READ | MODE_REALCREDS;
pub(crate) const MODE_ATTACH_FSCREDS: u32 = MODE_ATTACH | MODE_FSCREDS;
pub(crate) const MODE_ATTACH_REALCREDS: u32 = MODE_ATTACH | MODE_REALCREDS;

impl Kernel {
    /// `ptrace_may_access`: whether the task `tracer` may access `target` in `mode`. Tasks of
    /// one thread group always may; otherwise the ids and dumpability (`__ptrace_may_access`),
    /// then the LSMs: capabilities (`cap_ptrace_access_check`), Yama for attaching
    /// (`yama_ptrace_access_check`), Landlock (`hook_ptrace_access_check`). A task that is gone
    /// may not be accessed.
    pub(crate) fn ptrace_may_access(&self, tracer: TaskId, target: TaskId, mode: u32) -> bool {
        let (me, them) = {
            let state = self.state.read_or_recover();
            match (state.tasks.get(&tracer), state.tasks.get(&target)) {
                (Some(me), Some(them)) => (me.process.clone(), them.process.clone()),
                _ => return false,
            }
        };
        self.process_may_access(&me, &them, mode)
    }

    /// [`Kernel::ptrace_may_access`] between processes.
    pub(crate) fn process_may_access(
        &self,
        me: &Arc<Process>,
        them: &Arc<Process>,
        mode: u32,
    ) -> bool {
        if Arc::ptr_eq(me, them) {
            return true;
        }
        let (cred, tcred) = (me.cred(), them.cred());
        let dumpable = them.ident.lock_or_recover().dumpable;
        if !cred.may_read_task(&tcred, dumpable, mode & MODE_FSCREDS != 0) {
            return false;
        }
        // `cap_ptrace_access_check`: the target's permitted capabilities are the caller's in
        // the same user namespace, or the caller has `CAP_SYS_PTRACE` over the target's.
        let caller_caps = match mode & MODE_FSCREDS != 0 {
            true => cred.effective,
            false => cred.permitted,
        };
        let subset =
            Arc::ptr_eq(&cred.user_ns, &tcred.user_ns) && tcred.permitted & !caller_caps == 0;
        let ptrace_cap = cred.ns_capable(&tcred.user_ns, Cap::SysPtrace);
        if !subset && !ptrace_cap {
            return false;
        }
        if mode & MODE_ATTACH != 0 && !self.yama_may_attach(me, them, ptrace_cap) {
            return false;
        }
        Domain::may_ptrace(cred.landlock.as_ref(), tcred.landlock.as_ref())
    }

    /// `yama_ptrace_access_check` for an attach of `me` to `them`; `ptrace_cap` is whether `me`
    /// has `CAP_SYS_PTRACE` over `them`'s user namespace.
    fn yama_may_attach(&self, me: &Process, them: &Process, ptrace_cap: bool) -> bool {
        match self.yama.scope() {
            yama::SCOPE_DISABLED => true,
            yama::SCOPE_RELATIONAL => {
                ptrace_cap
                    || self.is_descendant(me.pid.0, them.pid.0)
                    || self.ptracer_exception_found(me.pid.0, them.pid.0)
            }
            yama::SCOPE_CAPABILITY => ptrace_cap,
            _ => false,
        }
    }

    /// `yama_ptrace_traceme`: whether `parent` may become the tracer of `me` through
    /// `PTRACE_TRACEME`, and Landlock's `hook_ptrace_traceme`.
    pub(crate) fn may_traceme(&self, me: &Process, parent: &Process) -> bool {
        let (cred, pcred) = (me.cred(), parent.cred());
        let yama = match self.yama.scope() {
            yama::SCOPE_CAPABILITY => pcred.ns_capable(&cred.user_ns, Cap::SysPtrace),
            yama::SCOPE_NO_ATTACH => false,
            _ => true,
        };
        yama && Domain::may_ptrace(pcred.landlock.as_ref(), cred.landlock.as_ref())
    }

    /// `task_is_descendant`: whether the process `child` is `ancestor` or below it, by its
    /// parents.
    pub(crate) fn is_descendant(&self, ancestor: u32, child: u32) -> bool {
        let tree = self.procs.tree.lock_or_recover();
        let mut walker = child;
        while walker != 0 {
            if walker == ancestor {
                return true;
            }
            walker = tree.nodes.get(&walker).map_or(0, |node| node.parent);
        }
        false
    }

    /// `ptracer_exception_found`: whether `tracee` named `tracer`, or one of its ancestors, or
    /// anyone, with `PR_SET_PTRACER`.
    fn ptracer_exception_found(&self, tracer: u32, tracee: u32) -> bool {
        match self.yama.exception(tracee) {
            Some(None) => true,
            Some(Some(allowed)) => self.is_descendant(allowed, tracer),
            None => false,
        }
    }
}

impl Kernel {
    /// `yama_task_prctl`'s `PR_SET_PTRACER` for the caller's thread group: 0 forgets the
    /// exception, `PR_SET_PTRACER_ANY` allows anyone, a pid allows that task's process and its
    /// descendants (`EINVAL` for a pid the caller cannot see).
    pub(crate) fn yama_set_ptracer(
        &self,
        caller: &wasixv2_syscalls::Caller<'_>,
        arg2: u32,
    ) -> wasixv2_syscalls::SysResult<i32> {
        let me = self.process(caller.task()).pid.0;
        match arg2 {
            0 => self.yama.del(me),
            u32::MAX => self.yama.add(None, me),
            pid => {
                // `find_get_task_by_vpid`: any task the caller sees.
                let tracer = (pid as i32 > 0)
                    .then(|| self.find_vpid(caller, pid as i32))
                    .flatten()
                    .and_then(|tid| self.task_process(TaskId(tid)))
                    .ok_or(wasixv2_abi::Errno::EINVAL)?;
                self.yama.add(Some(tracer.pid.0), me);
            }
        }
        Ok(0)
    }
}

impl Kernel {
    /// The process of `task`, if the task is in the table.
    pub(crate) fn task_process(&self, task: TaskId) -> Option<Arc<Process>> {
        let state = self.state.read_or_recover();
        state.tasks.get(&task).map(|t| t.process.clone())
    }
}

/// `PIDFD_SELF_THREAD`: a pidfd argument that names the calling thread.
const PIDFD_SELF_THREAD: i32 = -10000;
/// `PIDFD_SELF_THREAD_GROUP`: a pidfd argument that names the caller's thread group leader.
const PIDFD_SELF_THREAD_GROUP: i32 = -10001;

impl Kernel {
    /// `pidfd_get_task`: the task a pidfd argument names: the caller or its leader for
    /// `PIDFD_SELF_*`, else the thread group leader of the pidfd's process (`ESRCH` once it
    /// is gone).
    pub(crate) fn pidfd_get_task(
        &self,
        caller: &wasixv2_syscalls::Caller<'_>,
        pidfd: wasixv2_abi::Fd,
    ) -> Result<TaskId, wasixv2_abi::Errno> {
        let task = match pidfd.0 {
            PIDFD_SELF_THREAD => caller.task(),
            PIDFD_SELF_THREAD_GROUP => self.process(caller.task()).pid,
            _ => TaskId(self.pidfd_pid(caller, pidfd)?),
        };
        match self.task_process(task) {
            Some(_) => Ok(task),
            None => Err(wasixv2_abi::Errno::ESRCH),
        }
    }

    /// `mm_access`: `task`'s address space, for a caller in it or one that may access the
    /// task in `mode` (or, to read, has `CAP_PERFMON`: `perfmon_capable`); `EACCES` otherwise,
    /// `ESRCH` for a task that left its memory.
    pub(crate) fn mm_access(
        &self,
        caller: TaskId,
        task: TaskId,
        mode: u32,
    ) -> Result<Arc<crate::mm::Mm>, wasixv2_abi::Errno> {
        let (mm, mine) = {
            let state = self.state.read_or_recover();
            let t = state.tasks.get(&task).ok_or(wasixv2_abi::Errno::ESRCH)?;
            if t.thread.left_mm() {
                return Err(wasixv2_abi::Errno::ESRCH);
            }
            let mine = state.tasks.get(&caller).map(|c| c.mm.clone());
            (t.mm.clone(), mine)
        };
        if mine.is_some_and(|mine| Arc::ptr_eq(&mine, &mm))
            || self.ptrace_may_access(caller, task, mode)
        {
            return Ok(mm);
        }
        let perfmon = || {
            let cred = self.process(caller).cred();
            cred.capable_init(Cap::Perfmon) || cred.capable_init(Cap::SysAdmin)
        };
        match mode & MODE_READ != 0 && perfmon() {
            true => Ok(mm),
            false => Err(wasixv2_abi::Errno::EACCES),
        }
    }
}

/// What [`access_remote_vm`] copies: out of the address space into the buffer, or into it.
pub(crate) enum Access<'a> {
    Read(&'a mut [u8]),
    Write(&'a [u8]),
}

/// `access_remote_vm`: copies between `buf` and the address space `mm` from `addr`, a page at a
/// time, up to the first page that faults, and returns how many bytes it copied: 0 for an
/// address space nobody else can reach (its memory is not shared, or its last user left it).
/// With `force` (`FOLL_FORCE`, as ptrace and `/proc/<pid>/mem` copy) the pages' protection does
/// not count, but what the host may not write (the low guard, read-only shared views) still
/// faults.
pub(crate) async fn access_remote_vm(
    mm: &crate::mm::Mm,
    addr: u32,
    buf: Access<'_>,
    force: bool,
) -> usize {
    let Some(memory) = mm.foreign() else {
        return 0;
    };
    let page = crate::mm::PAGE_SIZE;
    let len = match &buf {
        Access::Read(buf) => buf.len(),
        Access::Write(buf) => buf.len(),
    };
    let mut done = 0;
    let mut buf = buf;
    while done < len {
        let at = u64::from(addr) + done as u64;
        let Ok(at32) = u32::try_from(at) else {
            break;
        };
        let n = ((page - at % page) as usize).min(len - done);
        let copied = match (&mut buf, force) {
            (Access::Read(buf), false) => memory.read(at32, &mut buf[done..done + n]).await,
            (Access::Read(buf), true) => memory.force_read(at32, &mut buf[done..done + n]).await,
            (Access::Write(buf), false) => memory.write(at32, &buf[done..done + n]).await,
            (Access::Write(buf), true) => memory.force_write(at32, &buf[done..done + n]).await,
        };
        if copied.is_err() {
            break;
        }
        done += n;
    }
    done
}
