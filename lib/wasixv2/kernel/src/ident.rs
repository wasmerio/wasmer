//! Process identity: ids and credentials, sessions and process groups, the task name and the
//! other per-process knobs `prctl` reads and writes, `uname` and `personality`.
//!
//! Every process runs as uid and gid 1 without capabilities, so the set*id calls can only
//! "change" an id to the value it already has.

use wasixv2_abi::{
    ConstBuf, Gid, GidArray, NewUtsname, NewUtsnamePtr, Pid, PrctlOption, Uid, UidPtr, Ulong,
};
use wasixv2_syscalls::{Caller, Errno, SysResult, read_value, write_value};

use crate::{ID, Kernel, Process};

/// `uname` fields. The release is the wasixv2 ABI version, not a Linux version: the target is not
/// Linux, and programs that parse the release to guess Linux features should not match it.
const SYSNAME: &str = "wasixv2";
const NODENAME: &str = "localhost";
const RELEASE: &str = "2.0.0";
const VERSION: &str = "#1";
const MACHINE: &str = "wasm32";
const DOMAINNAME: &str = "(none)";

/// `TASK_COMM_LEN`.
const COMM_LEN: usize = 16;
/// The default timer slack, in nanoseconds.
const TIMER_SLACK: u32 = 50_000;
/// `CAP_LAST_CAP`.
const CAP_LAST_CAP: u32 = 40;
/// `_NSIG`.
const NSIG: u32 = 64;
const PR_SET_VMA: PrctlOption = PrctlOption(0x53564d41);
const PR_CAP_AMBIENT: PrctlOption = PrctlOption(47);

/// What `prctl` and friends keep per process.
// TODO: the name, timer slack and nice value are per thread on Linux.
#[derive(Clone)]
pub(crate) struct Ident {
    pub(crate) pgid: u32,
    pub(crate) sid: u32,
    comm: [u8; COMM_LEN],
    dumpable: bool,
    pdeathsig: u32,
    no_new_privs: bool,
    timer_slack: u32,
    child_subreaper: bool,
    keepcaps: bool,
    pub(crate) personality: u32,
}

impl Ident {
    /// A session and group leader named after `argv[0]`.
    pub(crate) fn new(pid: u32, args: &[String]) -> Self {
        let mut comm = [0; COMM_LEN];
        let name = args.first().map(String::as_str).unwrap_or_default();
        let name = name.rsplit('/').next().unwrap_or_default().as_bytes();
        let n = name.len().min(COMM_LEN - 1);
        comm[..n].copy_from_slice(&name[..n]);
        Self {
            pgid: pid,
            sid: pid,
            comm,
            dumpable: true,
            pdeathsig: 0,
            no_new_privs: false,
            timer_slack: TIMER_SLACK,
            child_subreaper: false,
            keepcaps: false,
            personality: 0,
        }
    }
}

impl Ident {
    // ---- proc ----
    /// `execve` names the task after the file it runs (`kbasename(bprm->filename)`).
    pub(crate) fn set_comm_from_path(&mut self, path: &[u8]) {
        let name = path.rsplit(|&b| b == b'/').next().unwrap_or_default();
        let n = name.len().min(COMM_LEN - 1);
        self.comm = [0; COMM_LEN];
        self.comm[..n].copy_from_slice(&name[..n]);
    }

    /// `PR_SET_CHILD_SUBREAPER`.
    pub(crate) fn child_subreaper(&self) -> bool {
        self.child_subreaper
    }

    /// What a new process inherits (`copy_process`): everything but the parent-death signal
    /// and the subreaper flag.
    pub(crate) fn for_child(&self) -> Self {
        Self {
            pdeathsig: 0,
            child_subreaper: false,
            ..self.clone()
        }
    }
}

/// An unprivileged id change: each new id is -1 (unchanged) or the current id.
fn check_ids(ids: &[u32]) -> SysResult<i32> {
    match ids.iter().all(|&id| id == u32::MAX || id == ID as u32) {
        true => Ok(0),
        false => Err(Errno::EPERM.into()),
    }
}

fn utsname_field(s: &str) -> [u8; 65] {
    let mut out = [0; 65];
    out[..s.len()].copy_from_slice(s.as_bytes());
    out
}

impl Kernel {
    /// The process `pid` names for the caller: 0 is the caller's own.
    pub(crate) fn lookup_pid(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
    ) -> Result<std::sync::Arc<Process>, Errno> {
        match pid {
            0 => Ok(self.process(caller.task())),
            pid if pid < 0 => Err(Errno::EINVAL),
            // ---- proc ----
            pid => self.procs.find(pid as u32).ok_or(Errno::ESRCH),
        }
    }

    pub(crate) fn sys_set_ids(&self, ids: &[u32]) -> SysResult<i32> {
        check_ids(ids)
    }

    /// `setfsuid` and `setfsgid` return the old id whether or not they change it.
    pub(crate) fn sys_setfsid(&self) -> SysResult<i32> {
        Ok(ID)
    }

    pub(crate) async fn sys_getresid(
        &self,
        caller: &Caller<'_>,
        ptrs: [UidPtr; 3],
    ) -> SysResult<i32> {
        for ptr in ptrs {
            write_value(caller.memory(), ptr, &(ID as Uid)).await?;
        }
        Ok(0)
    }

    /// The only supplementary group is the primary one.
    pub(crate) async fn sys_getgroups(
        &self,
        caller: &Caller<'_>,
        size: i32,
        list: GidArray,
    ) -> SysResult<i32> {
        match size {
            ..0 => Err(Errno::EINVAL.into()),
            0 => Ok(1),
            _ => {
                write_value(caller.memory(), list, &(ID as Gid)).await?;
                Ok(1)
            }
        }
    }

    pub(crate) fn sys_getpgid(&self, caller: &Caller<'_>, pid: Pid) -> SysResult<Pid> {
        Ok(self.lookup_pid(caller, pid)?.ident.lock().unwrap().pgid as Pid)
    }

    pub(crate) fn sys_getsid(&self, caller: &Caller<'_>, pid: Pid) -> SysResult<Pid> {
        Ok(self.lookup_pid(caller, pid)?.ident.lock().unwrap().sid as Pid)
    }

    // TODO: the nodename and domainname come from the UTS namespace once `sethostname` exists.
    pub(crate) async fn sys_uname(
        &self,
        caller: &Caller<'_>,
        name: NewUtsnamePtr,
    ) -> SysResult<i32> {
        let uts = NewUtsname {
            sysname: utsname_field(SYSNAME),
            nodename: utsname_field(NODENAME),
            release: utsname_field(RELEASE),
            version: utsname_field(VERSION),
            machine: utsname_field(MACHINE),
            domainname: utsname_field(DOMAINNAME),
        };
        write_value(caller.memory(), name, &uts).await?;
        Ok(0)
    }

    /// Changing the UTS names needs `CAP_SYS_ADMIN`.
    pub(crate) fn sys_sethostname(&self, _: ConstBuf, _: i32) -> SysResult<i32> {
        Err(Errno::EPERM.into())
    }

    pub(crate) fn sys_personality(&self, caller: &Caller<'_>, personality: u32) -> SysResult<i32> {
        let process = self.process(caller.task());
        let mut ident = process.ident.lock().unwrap();
        let old = ident.personality;
        if personality != u32::MAX {
            ident.personality = personality;
        }
        Ok(old as i32)
    }

    pub(crate) async fn sys_prctl(
        &self,
        caller: &Caller<'_>,
        option: PrctlOption,
        arg2: Ulong,
        arg3: Ulong,
        arg4: Ulong,
        arg5: Ulong,
    ) -> SysResult<i32> {
        let process = self.process(caller.task());
        let memory = caller.memory();
        let put_int = |value: u32| async move {
            write_value(memory, wasixv2_abi::GuestPtrMut::<u32>::new(arg2), &value).await
        };
        let flag = |value: Ulong| match value {
            0 | 1 => Ok(value == 1),
            _ => Err(Errno::EINVAL),
        };
        match option {
            PrctlOption::PR_SET_NAME => {
                let mut name = [0; COMM_LEN];
                for (i, byte) in name.iter_mut().take(COMM_LEN - 1).enumerate() {
                    let ptr = wasixv2_abi::GuestPtr::<u8>::new(arg2 + i as u32);
                    *byte = read_value(memory, ptr).await?;
                    if *byte == 0 {
                        break;
                    }
                }
                process.ident.lock().unwrap().comm = name;
            }
            PrctlOption::PR_GET_NAME => {
                let comm = process.ident.lock().unwrap().comm;
                memory.write(arg2, &comm).await?;
            }
            PrctlOption::PR_GET_DUMPABLE => {
                return Ok(process.ident.lock().unwrap().dumpable as i32);
            }
            PrctlOption::PR_SET_DUMPABLE => process.ident.lock().unwrap().dumpable = flag(arg2)?,
            PrctlOption::PR_SET_PDEATHSIG => {
                if arg2 > NSIG {
                    return Err(Errno::EINVAL.into());
                }
                process.ident.lock().unwrap().pdeathsig = arg2;
            }
            PrctlOption::PR_GET_PDEATHSIG => {
                let sig = process.ident.lock().unwrap().pdeathsig;
                put_int(sig).await?;
            }
            PrctlOption::PR_SET_NO_NEW_PRIVS => {
                if arg2 != 1 || arg3 | arg4 | arg5 != 0 {
                    return Err(Errno::EINVAL.into());
                }
                process.ident.lock().unwrap().no_new_privs = true;
            }
            PrctlOption::PR_GET_NO_NEW_PRIVS => {
                if arg2 | arg3 | arg4 | arg5 != 0 {
                    return Err(Errno::EINVAL.into());
                }
                return Ok(process.ident.lock().unwrap().no_new_privs as i32);
            }
            PrctlOption::PR_SET_TIMERSLACK => {
                let slack = if arg2 == 0 { TIMER_SLACK } else { arg2 };
                process.ident.lock().unwrap().timer_slack = slack;
            }
            PrctlOption::PR_GET_TIMERSLACK => {
                return Ok(process.ident.lock().unwrap().timer_slack as i32);
            }
            PrctlOption::PR_SET_CHILD_SUBREAPER => {
                process.ident.lock().unwrap().child_subreaper = arg2 != 0;
            }
            PrctlOption::PR_GET_CHILD_SUBREAPER => {
                let reaper = process.ident.lock().unwrap().child_subreaper;
                put_int(reaper as u32).await?;
            }
            PrctlOption::PR_GET_KEEPCAPS => {
                return Ok(process.ident.lock().unwrap().keepcaps as i32);
            }
            PrctlOption::PR_SET_KEEPCAPS => process.ident.lock().unwrap().keepcaps = flag(arg2)?,
            PrctlOption::PR_CAPBSET_READ => {
                return match arg2 <= CAP_LAST_CAP {
                    true => Ok(1),
                    false => Err(Errno::EINVAL.into()),
                };
            }
            PrctlOption::PR_GET_SECUREBITS
            | PrctlOption::PR_GET_TIMING
            | PrctlOption::PR_GET_THP_DISABLE
            | PrctlOption::PR_GET_SECCOMP => return Ok(0),
            PrctlOption::PR_CAPBSET_DROP
            | PrctlOption::PR_SET_SECUREBITS
            | PrctlOption::PR_SET_MM => {
                return Err(Errno::EPERM.into());
            }
            PrctlOption::PR_SET_THP_DISABLE | PR_SET_VMA | PrctlOption::PR_SET_PTRACER => {}
            PR_CAP_AMBIENT => match arg2 {
                1 | 3 | 4 => {}
                2 => return Err(Errno::EPERM.into()),
                _ => return Err(Errno::EINVAL.into()),
            },
            _ => return Err(Errno::EINVAL.into()),
        }
        Ok(0)
    }
}
