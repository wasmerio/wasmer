//! Process identity: ids, credentials and capabilities, sessions and process groups, the task
//! name and the other knobs `prctl` reads and writes, `uname` and `personality`.
//!
//! The credential rules themselves live in [`crate::cred`]; this is the syscall side.

use std::fmt;

use wasixv2_abi::{
    ConstBuf, ConstGidArray, ConstUserCapDataArray, Gid, GidArray, GuestPtr, GuestPtrMut,
    NewUtsname, NewUtsnamePtr, Pid, PrctlArg, PrctlOption, Uid, UidPtr, Ulong, UserCapDataArray,
    UserCapDataStruct, UserCapHeaderPtr, UserCapHeaderStruct,
};
use wasixv2_syscalls::{Caller, Errno, SysErr, SysResult, read_value, write_value};
use wasixv2_vfs::Cap;

use crate::{
    Kernel, Process,
    cred::{Cred, IdChange, IdKind, NGROUPS_MAX, secure},
    startup::Identity,
    sync::MutexExt,
};

/// Linux task flags controlled by prctl; copied on clone and preserved on exec.
#[derive(Clone, Copy, Default)]
pub(crate) struct TaskPrctl {
    io_flusher: bool,
    /// PF_MCE_PROCESS/PF_MCE_EARLY: None selects the system default.
    mce_early: Option<bool>,
}

/// Linux mm flags and private-futex hash tuning. The hash size is advisory:
/// the kernel's own futex buckets remain an implementation detail.
#[derive(Clone, Copy, Default)]
pub(crate) struct MmPrctl {
    mdwe: u32,
    thp_disable: u32,
    futex_slots: Option<u32>,
}

impl MmPrctl {
    pub(crate) fn for_fork(&self) -> Self {
        Self {
            mdwe: if self.mdwe & PrctlArg::PR_MDWE_NO_INHERIT.0 != 0 {
                0
            } else {
                self.mdwe
            },
            thp_disable: self.thp_disable,
            // Linux futex_hash_init_mm clears the child's custom hash.
            futex_slots: None,
        }
    }
}

/// `TASK_COMM_LEN`.
const COMM_LEN: usize = 16;
/// `_NSIG`.
const NSIG: u32 = 64;
const PR_SET_VMA: PrctlOption = PrctlOption(0x53564d41);
const PR_CAP_AMBIENT: PrctlOption = PrctlOption(47);
/// `__NEW_UTS_LEN`.
const UTS_LEN: usize = 64;
/// `_LINUX_CAPABILITY_VERSION_1`, `_2` and `_3`: version 1 has one 32-bit word per set, the
/// others two.
const CAPABILITY_V1: u32 = 0x1998_0330;
const CAPABILITY_V2: u32 = 0x2007_1026;
const CAPABILITY_V3: u32 = 0x2008_0522;

/// The names `uname` reports (`struct new_utsname`).
///
/// The defaults are `Linux` `localhost` `7.2.0-wasixv2` `#1 SMP wasixv2 2.0.0` `wasm32`, with
/// the domain name `(none)`. Programs are built for Linux (`__linux__`), so the system name is
/// Linux's: configure scripts and programs that pick their code by `uname -s` take the Linux
/// paths. The release is the Linux version of the UAPI headers libc2 ships
/// ([`Uname::LINUX_RELEASE`]) with a `-wasixv2` suffix, as a distribution kernel names its
/// flavour (`6.8.0-117-generic`): programs compare the version part with the versions those
/// headers document, so an older one sends them down workarounds for old kernels, and a newer
/// one would claim interfaces their headers do not have. What the kernel does not implement
/// is `ENOSYS`, as on a Linux built without it. The suffix and the version, which names the
/// wasixv2 ABI, let a program still tell. An embedder that runs programs which insist on
/// another release can report one. The system name is always `Linux`. A guest with
/// `CAP_SYS_ADMIN` can change the node and domain names with `sethostname` and
/// `setdomainname`. WASIX v1 programs never ask the kernel: their libc answers `uname`
/// itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Uname {
    nodename: String,
    release: String,
    version: String,
    machine: String,
    domainname: String,
}

impl Default for Uname {
    fn default() -> Self {
        Self {
            nodename: "localhost".into(),
            release: Self::RELEASE.into(),
            version: Self::VERSION.into(),
            machine: "wasm32".into(),
            domainname: "(none)".into(),
        }
    }
}

/// A `uname` field that does not fit `struct new_utsname`: longer than 64 bytes, or containing
/// a NUL byte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnameError {
    field: &'static str,
}

impl fmt::Display for UnameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the uname {} must be at most {UTS_LEN} bytes and contain no NUL byte",
            self.field
        )
    }
}

impl std::error::Error for UnameError {}

fn uts_value(field: &'static str, value: String) -> Result<String, UnameError> {
    match value.len() <= UTS_LEN && !value.contains('\0') {
        true => Ok(value),
        false => Err(UnameError { field }),
    }
}

impl Uname {
    /// The system name, which is always `Linux`.
    pub const SYSNAME: &str = "Linux";

    /// The Linux version of the UAPI headers in libc2's sysroot (`LINUX_VERSION_CODE` in
    /// `linux/version.h`).
    pub const LINUX_RELEASE: &str = "7.2.0";

    /// The default release: [`Uname::LINUX_RELEASE`] with the wasixv2 flavour.
    pub const RELEASE: &str = "7.2.0-wasixv2";

    /// The default version: a build number and `SMP` as Linux's, then the wasixv2 ABI
    /// version.
    pub const VERSION: &str = "#1 SMP wasixv2 2.0.0";

    /// `uname -n`, the host name.
    pub fn with_nodename(mut self, value: impl Into<String>) -> Result<Self, UnameError> {
        self.nodename = uts_value("node name", value.into())?;
        Ok(self)
    }

    /// `uname -r`.
    pub fn with_release(mut self, value: impl Into<String>) -> Result<Self, UnameError> {
        self.release = uts_value("release", value.into())?;
        Ok(self)
    }

    /// `uname -v`.
    pub fn with_version(mut self, value: impl Into<String>) -> Result<Self, UnameError> {
        self.version = uts_value("version", value.into())?;
        Ok(self)
    }

    /// `uname -m`.
    pub fn with_machine(mut self, value: impl Into<String>) -> Result<Self, UnameError> {
        self.machine = uts_value("machine", value.into())?;
        Ok(self)
    }

    /// The NIS domain name (`getdomainname`).
    pub fn with_domainname(mut self, value: impl Into<String>) -> Result<Self, UnameError> {
        self.domainname = uts_value("domain name", value.into())?;
        Ok(self)
    }

    pub fn nodename(&self) -> &str {
        &self.nodename
    }

    pub fn release(&self) -> &str {
        &self.release
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn machine(&self) -> &str {
        &self.machine
    }

    pub fn domainname(&self) -> &str {
        &self.domainname
    }

    /// The fields as the guest sees them.
    pub(crate) fn to_utsname(&self) -> NewUtsname {
        NewUtsname {
            sysname: utsname_field(Self::SYSNAME.as_bytes()),
            nodename: utsname_field(self.nodename.as_bytes()),
            release: utsname_field(self.release.as_bytes()),
            version: utsname_field(self.version.as_bytes()),
            machine: utsname_field(self.machine.as_bytes()),
            domainname: utsname_field(self.domainname.as_bytes()),
        }
    }
}

/// A task's name, NUL-padded (`task_struct.comm`). Each thread has its own.
#[derive(Clone, Copy)]
pub(crate) struct Comm([u8; COMM_LEN]);

impl Comm {
    /// `name` cut to `TASK_COMM_LEN - 1` bytes, as `set_task_comm` takes it.
    pub(crate) fn new(name: &[u8]) -> Self {
        let mut comm = [0; COMM_LEN];
        let n = name.len().min(COMM_LEN - 1);
        comm[..n].copy_from_slice(&name[..n]);
        Self(comm)
    }

    /// The name `execve` gives a task: the file's base name (`kbasename(bprm->filename)`).
    pub(crate) fn from_path(path: &[u8]) -> Self {
        Self::new(path.rsplit(|&b| b == b'/').next().unwrap_or_default())
    }

    /// The name, NUL-padded to `TASK_COMM_LEN`.
    pub(crate) fn bytes(&self) -> [u8; COMM_LEN] {
        self.0
    }

    /// The name without its NUL padding.
    pub(crate) fn name(&self) -> &[u8] {
        let n = self.0.iter().position(|&b| b == 0).unwrap_or(COMM_LEN);
        &self.0[..n]
    }
}

/// `get_dumpable` (`SUID_DUMP_*`): whether the process dumps core, as its user or as root, and
/// whether its `/proc` files and its memory are its user's to look at (only `User`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dumpable {
    Disable,
    User,
    Root,
}

impl Dumpable {
    /// What `fs.suid_dumpable` makes a process that changed its credentials.
    pub(crate) fn suid(tunables: &crate::sysctl::Tunables) -> Self {
        match tunables.get(crate::sysctl::Tunable::SuidDumpable) {
            1 => Dumpable::User,
            2 => Dumpable::Root,
            _ => Dumpable::Disable,
        }
    }

    pub(crate) fn value(self) -> i32 {
        match self {
            Dumpable::Disable => 0,
            Dumpable::User => 1,
            Dumpable::Root => 2,
        }
    }
}

/// What `prctl` and friends keep per process.
#[derive(Clone)]
pub(crate) struct Ident {
    pub(crate) pgid: u32,
    pub(crate) sid: u32,
    pub(crate) dumpable: Dumpable,
    pdeathsig: u32,
    child_subreaper: bool,
    pub(crate) personality: u32,
    /// Whether the current image was started privilege-elevating (`AT_SECURE`).
    pub(crate) secure_exec: bool,
    /// The controlling terminal (`signal->tty`): a child inherits it, `execve` keeps it,
    /// `setsid` drops it.
    pub(crate) tty: Option<std::sync::Arc<crate::tty::Tty>>,
    /// `signal->oom_score_adj` and `oom_score_adj_min`: what the OOM killer adds to the
    /// process's badness, from -1000 (never) to 1000, and how low it may go without
    /// `CAP_SYS_RESOURCE`. Inherited.
    pub(crate) oom_score_adj: i32,
    pub(crate) oom_score_adj_min: i32,
    /// The audit login uid (`loginuid`), `None` unset, and session id (`sessionid`,
    /// `AUDIT_SID_UNSET` unset). Inherited.
    pub(crate) loginuid: Option<crate::uidgid::Kuid>,
    pub(crate) sessionid: u32,
    /// The process dumped core (`PF_DUMPCORE`), which its accounting record tells.
    pub(crate) dumped_core: bool,
    /// The task that took a signal that dumps core, which dumps on its way out, and the
    /// signal.
    pub(crate) core_dumper: Option<(wasixv2_syscalls::TaskId, wasixv2_abi::Siginfo)>,
    /// `coredump_filter` (`mm->flags`' dump bits): a child inherits it, `execve` keeps it.
    pub(crate) coredump_filter: u32,
}

impl Ident {
    /// A session and group leader.
    pub(crate) fn new(pid: u32) -> Self {
        Self {
            pgid: pid,
            sid: pid,
            dumpable: Dumpable::User,
            pdeathsig: 0,
            child_subreaper: false,
            personality: 0,
            secure_exec: false,
            tty: None,
            oom_score_adj: 0,
            oom_score_adj_min: 0,
            loginuid: None,
            sessionid: u32::MAX,
            dumped_core: false,
            core_dumper: None,
            coredump_filter: crate::coredump::COREDUMP_FILTER_DEFAULT,
        }
    }
}

impl Ident {
    /// `PR_SET_CHILD_SUBREAPER`.
    pub(crate) fn child_subreaper(&self) -> bool {
        self.child_subreaper
    }

    /// `PR_SET_PDEATHSIG`: the signal the process gets when its parent exits, 0 for none.
    pub(crate) fn pdeathsig(&self) -> i32 {
        self.pdeathsig as i32
    }

    /// Forgets the parent-death signal, so a parent cannot signal a process that became more
    /// privileged than it (`commit_creds`, and a set-user-ID `execve`).
    pub(crate) fn clear_pdeathsig(&mut self) {
        self.pdeathsig = 0;
    }

    /// What a new process inherits (`copy_process`): everything but the parent-death signal
    /// and the subreaper flag.
    pub(crate) fn for_child(&self) -> Self {
        Self {
            pdeathsig: 0,
            child_subreaper: false,
            dumped_core: false,
            core_dumper: None,
            ..self.clone()
        }
    }
}

fn utsname_field(s: &[u8]) -> [u8; 65] {
    let mut out = [0; 65];
    out[..s.len()].copy_from_slice(s);
    out
}

/// `cap_valid`.
fn cap_valid(cap: Ulong) -> bool {
    cap <= Cap::LAST
}

impl Process {
    /// A snapshot of the process's credentials.
    pub(crate) fn cred(&self) -> Cred {
        self.cred.lock_or_recover().clone()
    }

    /// `commit_creds`: installs `new`, and makes the process as dumpable as `fs.suid_dumpable`
    /// says and forgets its parent-death signal if that changed its effective or filesystem
    /// ids or gave it capabilities.
    pub(crate) fn commit_cred(&self, new: Cred) {
        let mut cred = self.cred.lock_or_recover();
        if new.drops_dumpable(&cred) {
            let mut ident = self.ident.lock_or_recover();
            ident.dumpable = Dumpable::suid(&self.tunables);
            ident.clear_pdeathsig();
        }
        *cred = new;
    }

    /// The identity entries of the process's auxiliary vector, as its user namespace sees them
    /// (`create_elf_tables`).
    pub(crate) fn identity(&self) -> Identity {
        let cred = self.cred();
        let ns = &cred.user_ns;
        Identity {
            uid: ns.munged_uid(cred.user.real),
            euid: ns.munged_uid(cred.user.effective),
            gid: ns.munged_gid(cred.group.real),
            egid: ns.munged_gid(cred.group.effective),
            secure: self.ident.lock_or_recover().secure_exec,
        }
    }

    /// Applies `change` to the credentials, if it allows.
    pub(crate) fn update_cred(
        &self,
        change: impl FnOnce(&Cred) -> Result<Cred, Errno>,
    ) -> SysResult<i32> {
        let new = change(&self.cred())?;
        self.commit_cred(new);
        Ok(0)
    }
}

impl Kernel {
    /// The process of the task `pid` names for the caller (`find_task_by_vpid`): 0 is the
    /// caller's own. A thread's tid names its process too.
    pub(crate) fn lookup_pid(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
    ) -> Result<std::sync::Arc<Process>, Errno> {
        match pid {
            0 => Ok(self.process(caller.task())),
            pid => {
                let id = self.find_vpid(caller, pid).ok_or(Errno::ESRCH)?;
                // ---- proc ----
                if let Some(process) = self.procs.find(id) {
                    return Ok(process);
                }
                let process = {
                    let state = self.state.read_or_recover();
                    let task = state.tasks.get(&wasixv2_syscalls::TaskId(id));
                    task.map(|task| task.process.clone()).ok_or(Errno::ESRCH)?
                };
                // A task of a reaped thread group is on its way out, not a process.
                match self.procs.find(process.pid.0) {
                    Some(_) => Ok(process),
                    None => Err(Errno::ESRCH),
                }
            }
        }
    }

    /// The caller's credentials.
    pub(crate) fn cred(&self, caller: &Caller<'_>) -> Cred {
        self.process(caller.task()).cred()
    }

    /// `setuid`, `setreuid`, `setresuid` and their gid twins, with the guest's ids, which must
    /// be mapped in its user namespace (`EINVAL`).
    pub(crate) fn sys_set_ids(
        &self,
        caller: &Caller<'_>,
        kind: IdKind,
        change: IdChange<u32>,
    ) -> SysResult<i32> {
        self.process(caller.task()).update_cred(|cred| match kind {
            IdKind::User => cred.change_uids(change.to_kernel(|id| cred.user_ns.make_kuid(id))?),
            IdKind::Group => cred.change_gids(change.to_kernel(|id| cred.user_ns.make_kgid(id))?),
        })
    }

    /// `setfsuid` and `setfsgid` return the old id whether or not they change it.
    pub(crate) fn sys_setfsid(&self, caller: &Caller<'_>, kind: IdKind, id: u32) -> SysResult<i32> {
        let process = self.process(caller.task());
        let cred = process.cred();
        let ns = &cred.user_ns;
        let (old, new) = match kind {
            IdKind::User => {
                let (old, new) = cred.change_fsuid(ns.make_kuid(id));
                (ns.munged_uid(old), new)
            }
            IdKind::Group => {
                let (old, new) = cred.change_fsgid(ns.make_kgid(id));
                (ns.munged_gid(old), new)
            }
        };
        process.commit_cred(new);
        Ok(old as i32)
    }

    /// The caller's real, effective and saved uids or gids, as its user namespace sees them.
    fn ids_seen(&self, caller: &Caller<'_>, kind: IdKind) -> [u32; 3] {
        let cred = self.cred(caller);
        let ns = &cred.user_ns;
        match kind {
            IdKind::User => {
                let ids = cred.user;
                [ids.real, ids.effective, ids.saved].map(|id| ns.munged_uid(id))
            }
            IdKind::Group => {
                let ids = cred.group;
                [ids.real, ids.effective, ids.saved].map(|id| ns.munged_gid(id))
            }
        }
    }

    pub(crate) fn sys_getid(&self, caller: &Caller<'_>, kind: IdKind, effective: bool) -> i32 {
        let [real, euid, _] = self.ids_seen(caller, kind);
        match effective {
            true => euid as i32,
            false => real as i32,
        }
    }

    pub(crate) async fn sys_getresid(
        &self,
        caller: &Caller<'_>,
        kind: IdKind,
        ptrs: [UidPtr; 3],
    ) -> SysResult<i32> {
        let ids = self.ids_seen(caller, kind);
        for (ptr, id) in ptrs.into_iter().zip(ids) {
            write_value(caller.memory(), ptr, &(id as Uid)).await?;
        }
        Ok(0)
    }

    pub(crate) async fn sys_getgroups(
        &self,
        caller: &Caller<'_>,
        size: i32,
        list: GidArray,
    ) -> SysResult<i32> {
        let cred = self.cred(caller);
        let groups: Vec<u32> = cred
            .groups
            .iter()
            .map(|&gid| cred.user_ns.munged_gid(gid))
            .collect();
        let count = groups.len() as i32;
        match size {
            ..0 => Err(Errno::EINVAL.into()),
            0 => Ok(count),
            size if size < count => Err(Errno::EINVAL.into()),
            _ => {
                let bytes: Vec<u8> = groups.iter().flat_map(|gid| gid.to_le_bytes()).collect();
                caller.memory().write(list.addr(), &bytes).await?;
                Ok(count)
            }
        }
    }

    /// `setgroups`: the privilege check ([`Cred::may_setgroups`]) comes before the size check.
    pub(crate) async fn sys_setgroups(
        &self,
        caller: &Caller<'_>,
        size: i32,
        list: ConstGidArray,
    ) -> SysResult<i32> {
        let process = self.process(caller.task());
        if !process.cred().may_setgroups() {
            return Err(Errno::EPERM.into());
        }
        let size = usize::try_from(size)
            .ok()
            .filter(|&size| size <= NGROUPS_MAX)
            .ok_or(Errno::EINVAL)?;
        let mut bytes = vec![0; size * 4];
        caller.memory().read(list.addr(), &mut bytes).await?;
        let groups: Vec<Gid> = bytes
            .chunks_exact(4)
            .map(|gid| Gid::from_le_bytes(gid.try_into().unwrap()))
            .collect();
        process.update_cred(|cred| {
            // `groups_from_user`: every group must be mapped.
            let groups = groups
                .iter()
                .map(|&gid| cred.user_ns.make_kgid(gid).ok_or(Errno::EINVAL))
                .collect::<Result<_, _>>()?;
            cred.set_groups(groups)
        })
    }

    /// `capget`: an unknown header version is answered with the preferred one.
    pub(crate) async fn sys_capget(
        &self,
        caller: &Caller<'_>,
        header: UserCapHeaderPtr,
        data: UserCapDataArray,
    ) -> SysResult<i32> {
        let words = match self.capability_words(caller, header).await {
            Err(SysErr::Errno(Errno::EINVAL)) if data.is_null() => return Ok(0),
            result => result?,
        };
        if data.is_null() {
            return Ok(0);
        }
        let hdr: UserCapHeaderStruct =
            read_value(caller.memory(), GuestPtr::new(header.addr())).await?;
        if hdr.pid < 0 {
            return Err(Errno::EINVAL.into());
        }
        let cred = self.lookup_pid(caller, hdr.pid)?.cred();
        for word in 0..words {
            let shift = 32 * word;
            let set = UserCapDataStruct {
                effective: (cred.effective >> shift) as u32,
                permitted: (cred.permitted >> shift) as u32,
                inheritable: (cred.inheritable >> shift) as u32,
            };
            let ptr = UserCapDataArray::new(data.addr() + word * 12);
            write_value(caller.memory(), ptr, &set).await?;
        }
        Ok(0)
    }

    /// `capset`: only the caller's own sets can change.
    pub(crate) async fn sys_capset(
        &self,
        caller: &Caller<'_>,
        header: UserCapHeaderPtr,
        data: ConstUserCapDataArray,
    ) -> SysResult<i32> {
        let words = self.capability_words(caller, header).await?;
        let hdr: UserCapHeaderStruct =
            read_value(caller.memory(), GuestPtr::new(header.addr())).await?;
        let process = self.process(caller.task());
        if hdr.pid != 0 && hdr.pid != self.own_tid(caller.task()) {
            return Err(Errno::EPERM.into());
        }
        let (mut effective, mut permitted, mut inheritable) = (0, 0, 0);
        for word in 0..words {
            let ptr = ConstUserCapDataArray::new(data.addr() + word * 12);
            let set = read_value(caller.memory(), ptr).await?;
            let shift = 32 * word;
            effective |= u64::from(set.effective) << shift;
            permitted |= u64::from(set.permitted) << shift;
            inheritable |= u64::from(set.inheritable) << shift;
        }
        process.update_cred(|cred| cred.capset(effective, permitted, inheritable))
    }

    /// `cap_validate_magic`: how many 32-bit words per set the header's version uses.
    async fn capability_words(
        &self,
        caller: &Caller<'_>,
        header: UserCapHeaderPtr,
    ) -> SysResult<u32> {
        let version = GuestPtr::<u32>::new(header.addr());
        match read_value(caller.memory(), version).await? {
            CAPABILITY_V1 => Ok(1),
            CAPABILITY_V2 | CAPABILITY_V3 => Ok(2),
            _ => {
                let version = GuestPtrMut::<u32>::new(header.addr());
                write_value(caller.memory(), version, &CAPABILITY_V3).await?;
                Err(Errno::EINVAL.into())
            }
        }
    }

    /// The process group as the caller sees it: 0 for one outside its pid namespace.
    pub(crate) fn sys_getpgid(&self, caller: &Caller<'_>, pid: Pid) -> SysResult<Pid> {
        let pgid = self.lookup_pid(caller, pid)?.ident.lock_or_recover().pgid;
        Ok(self.vnr(caller, pgid))
    }

    /// The session as the caller sees it: 0 for one outside its pid namespace.
    pub(crate) fn sys_getsid(&self, caller: &Caller<'_>, pid: Pid) -> SysResult<Pid> {
        let sid = self.lookup_pid(caller, pid)?.ident.lock_or_recover().sid;
        Ok(self.vnr(caller, sid))
    }

    pub(crate) async fn sys_uname(
        &self,
        caller: &Caller<'_>,
        name: NewUtsnamePtr,
    ) -> SysResult<i32> {
        let uts = *self.ns(caller).uts.lock_or_recover();
        write_value(caller.memory(), name, &uts).await?;
        Ok(0)
    }

    /// `sethostname` or, with `domain`, `setdomainname`: needs `CAP_SYS_ADMIN` over the UTS
    /// namespace's owner.
    pub(crate) async fn sys_sethostname(
        &self,
        caller: &Caller<'_>,
        name: ConstBuf,
        len: i32,
        domain: bool,
    ) -> SysResult<i32> {
        let owner = self.ns(caller).uts.user_ns().clone();
        if !self.cred(caller).ns_capable(&owner, Cap::SysAdmin) {
            return Err(Errno::EPERM.into());
        }
        let len = usize::try_from(len)
            .ok()
            .filter(|&len| len <= UTS_LEN)
            .ok_or(Errno::EINVAL)?;
        let mut value = vec![0; len];
        caller.memory().read(name.addr(), &mut value).await?;
        let ns = self.ns(caller);
        let mut uts = ns.uts.lock_or_recover();
        let field = match domain {
            true => &mut uts.domainname,
            false => &mut uts.nodename,
        };
        *field = utsname_field(&value);
        Ok(0)
    }

    pub(crate) fn sys_personality(&self, caller: &Caller<'_>, personality: u32) -> SysResult<i32> {
        let process = self.process(caller.task());
        let mut ident = process.ident.lock_or_recover();
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
                self.with_task(caller.task(), |t| t.comm = Comm(name));
            }
            PrctlOption::PR_GET_NAME => {
                let Comm(comm) = self.with_task(caller.task(), |t| t.comm);
                memory.write(arg2, &comm).await?;
            }
            PrctlOption::PR_GET_DUMPABLE => {
                return Ok(process.ident.lock_or_recover().dumpable.value());
            }
            // Only `SUID_DUMP_DISABLE` or `SUID_DUMP_USER`.
            PrctlOption::PR_SET_DUMPABLE => {
                process.ident.lock_or_recover().dumpable = match flag(arg2)? {
                    true => Dumpable::User,
                    false => Dumpable::Disable,
                }
            }
            PrctlOption::PR_SET_PDEATHSIG => {
                if arg2 > NSIG {
                    return Err(Errno::EINVAL.into());
                }
                process.ident.lock_or_recover().pdeathsig = arg2;
            }
            PrctlOption::PR_GET_PDEATHSIG => {
                let sig = process.ident.lock_or_recover().pdeathsig;
                put_int(sig).await?;
            }
            PrctlOption::PR_SET_NO_NEW_PRIVS => {
                if arg2 != 1 || arg3 | arg4 | arg5 != 0 {
                    return Err(Errno::EINVAL.into());
                }
                self.set_no_new_privs(caller);
            }
            PrctlOption::PR_GET_NO_NEW_PRIVS => {
                if arg2 | arg3 | arg4 | arg5 != 0 {
                    return Err(Errno::EINVAL.into());
                }
                return Ok(self.no_new_privs(caller) as i32);
            }
            PrctlOption::PR_SET_TIMERSLACK => {
                self.with_task(caller.task(), |t| t.sched.set_timer_slack(arg2.into()));
            }
            PrctlOption::PR_GET_TIMERSLACK => {
                // The result is a `long`.
                let slack = self.with_task(caller.task(), |t| t.sched.timer_slack());
                return Ok(slack.min(i32::MAX as u64) as i32);
            }
            PrctlOption::PR_SET_CHILD_SUBREAPER => {
                process.ident.lock_or_recover().child_subreaper = arg2 != 0;
            }
            PrctlOption::PR_GET_CHILD_SUBREAPER => {
                let reaper = process.ident.lock_or_recover().child_subreaper;
                put_int(reaper as u32).await?;
            }
            PrctlOption::PR_TIMER_CREATE_RESTORE_IDS => {
                return self.timer_create_prctl(caller, arg2, [arg3, arg4, arg5]);
            }
            PrctlOption::PR_GET_KEEPCAPS => {
                return Ok(process.cred().secure(secure::KEEP_CAPS) as i32);
            }
            PrctlOption::PR_SET_KEEPCAPS => {
                let keep = flag(arg2)?;
                return process.update_cred(|cred| {
                    if cred.secure(secure::KEEP_CAPS_LOCKED) {
                        return Err(Errno::EPERM);
                    }
                    let mut new = cred.clone();
                    new.securebits &= !secure::KEEP_CAPS;
                    new.securebits |= if keep { secure::KEEP_CAPS } else { 0 };
                    Ok(new)
                });
            }
            PrctlOption::PR_GET_SECUREBITS => return Ok(process.cred().securebits as i32),
            PrctlOption::PR_SET_SECUREBITS => {
                return process.update_cred(|cred| set_securebits(cred, arg2));
            }
            PrctlOption::PR_CAPBSET_READ => {
                if !cap_valid(arg2) {
                    return Err(Errno::EINVAL.into());
                }
                return Ok((process.cred().bounding >> arg2 & 1) as i32);
            }
            PrctlOption::PR_CAPBSET_DROP => {
                return process.update_cred(|cred| {
                    if !cred.capable(Cap::Setpcap) {
                        return Err(Errno::EPERM);
                    }
                    if !cap_valid(arg2) {
                        return Err(Errno::EINVAL);
                    }
                    let mut new = cred.clone();
                    new.bounding &= !(1 << arg2);
                    Ok(new)
                });
            }
            PR_CAP_AMBIENT => return ambient(&process, arg2, arg3, arg4 | arg5),
            PrctlOption::PR_GET_SECCOMP => return Ok(self.prctl_get_seccomp(caller)),
            PrctlOption::PR_SET_SECCOMP => {
                return self.prctl_set_seccomp(caller, arg2, arg3).await;
            }
            PrctlOption::PR_SET_IO_FLUSHER | PrctlOption::PR_GET_IO_FLUSHER => {
                // Linux kernel/sys.c:sys_prctl checks capable() before arguments.
                if !self.cred(caller).capable_init(Cap::SysResource) {
                    return Err(Errno::EPERM.into());
                }
                if arg3 | arg4 | arg5 != 0
                    || (option == PrctlOption::PR_GET_IO_FLUSHER && arg2 != 0)
                {
                    return Err(Errno::EINVAL.into());
                }
                if option == PrctlOption::PR_GET_IO_FLUSHER {
                    return Ok(self.task_ref(caller.task(), |t| t.prctl.io_flusher) as i32);
                }
                let value = flag(arg2)?;
                self.with_task(caller.task(), |t| t.prctl.io_flusher = value);
            }
            PrctlOption::PR_GET_AUXV => {
                if arg4 | arg5 != 0 {
                    return Err(Errno::EINVAL.into());
                }
                let mut auxv = process.auxv.lock_or_recover().clone();
                auxv.resize(crate::startup::SAVED_AUXV_SIZE, 0);
                let size = auxv.len().min(arg3 as usize);
                if size != 0 {
                    memory.write(arg2, &auxv[..size]).await?;
                }
                return Ok(auxv.len() as i32);
            }
            PrctlOption::PR_GET_TID_ADDRESS => {
                let addr = self.task_ref(caller.task(), |t| t.thread.tid_address());
                put_int(addr).await?;
            }
            PrctlOption::PR_MCE_KILL_GET => {
                if arg2 | arg3 | arg4 | arg5 != 0 {
                    return Err(Errno::EINVAL.into());
                }
                return Ok(self
                    .task_ref(caller.task(), |t| t.prctl.mce_early)
                    .map_or(PrctlArg::PR_MCE_KILL_DEFAULT.0 as i32, i32::from));
            }
            PrctlOption::PR_MCE_KILL => {
                if arg4 | arg5 != 0 {
                    return Err(Errno::EINVAL.into());
                }
                match arg2 {
                    0 if arg3 == 0 => self.with_task(caller.task(), |t| t.prctl.mce_early = None),
                    1 => {
                        // Linux sets PF_MCE_PROCESS before validating arg3, including
                        // on EINVAL; preserve that observable state transition.
                        self.with_task(caller.task(), |t| {
                            t.prctl.mce_early = Some(t.prctl.mce_early.unwrap_or(false))
                        });
                        let policy = match arg3 {
                            0 => Some(false),
                            1 => Some(true),
                            2 => None,
                            _ => return Err(Errno::EINVAL.into()),
                        };
                        self.with_task(caller.task(), |t| t.prctl.mce_early = policy);
                    }
                    _ => return Err(Errno::EINVAL.into()),
                }
            }
            PrctlOption::PR_SET_MDWE
            | PrctlOption::PR_GET_MDWE
            | PrctlOption::PR_FUTEX_HASH
            | PrctlOption::PR_SET_THP_DISABLE
            | PrctlOption::PR_GET_THP_DISABLE => {
                return self.mm_prctl(caller, option, [arg2, arg3, arg4, arg5]);
            }
            PrctlOption::PR_GET_TIMING => return Ok(0),
            PrctlOption::PR_SET_TIMING => {
                if arg2 != PrctlArg::PR_TIMING_STATISTICAL.0 {
                    return Err(Errno::EINVAL.into());
                }
            }
            // Linux include/linux/syscall_user_dispatch.h:set_syscall_user_dispatch
            // returns EINVAL with the feature disabled. Wasm has no syscall PC for
            // checking the permitted code range; see DESIGN.md's limits table.
            PrctlOption::PR_SET_SYSCALL_USER_DISPATCH => return Err(Errno::EINVAL.into()),
            // `prctl_set_mm` needs CAP_SYS_RESOURCE over the initial user namespace, and even
            // then the wasm memory layout has nothing it could change.
            PrctlOption::PR_SET_MM => {
                return match process.cred().capable_init(Cap::SysResource) {
                    true => Err(Errno::EINVAL.into()),
                    false => Err(Errno::EPERM.into()),
                };
            }
            PR_SET_VMA => {}
            PrctlOption::PR_SET_PTRACER => return self.yama_set_ptracer(caller, arg2),
            // No preemptive slice extensions on wasm; see DESIGN.md. The prctl
            // is deliberately unsupported while rseq_slice_yield returns zero.
            PrctlOption::PR_RSEQ_SLICE_EXTENSION => return Err(Errno::EINVAL.into()),
            _ => return Err(Errno::EINVAL.into()),
        }
        Ok(0)
    }
}

impl Kernel {
    /// Linux kernel/sys.c:prctl_{set,get}_mdwe, prctl_{set,get}_thp_disable
    /// and kernel/futex/core.c:futex_hash_prctl/futex_hash_allocate.
    fn mm_prctl(
        &self,
        caller: &Caller<'_>,
        op: PrctlOption,
        [arg2, arg3, arg4, arg5]: [u32; 4],
    ) -> SysResult<i32> {
        let mm = self.mm(caller.task());
        let mut state = mm.prctl.lock_or_recover();
        match op {
            PrctlOption::PR_GET_MDWE | PrctlOption::PR_GET_THP_DISABLE => {
                if arg2 | arg3 | arg4 | arg5 != 0 {
                    return Err(Errno::EINVAL.into());
                }
                return Ok(if op == PrctlOption::PR_GET_MDWE {
                    state.mdwe
                } else {
                    state.thp_disable
                } as i32);
            }
            PrctlOption::PR_SET_MDWE => {
                let refuse = PrctlArg::PR_MDWE_REFUSE_EXEC_GAIN.0;
                let no_inherit = PrctlArg::PR_MDWE_NO_INHERIT.0;
                if arg3 | arg4 | arg5 != 0
                    || arg2 & !(refuse | no_inherit) != 0
                    || (arg2 & no_inherit != 0 && arg2 & refuse == 0)
                {
                    return Err(Errno::EINVAL.into());
                }
                if state.mdwe != 0 && state.mdwe != arg2 {
                    return Err(Errno::EPERM.into());
                }
                state.mdwe = arg2;
            }
            PrctlOption::PR_SET_THP_DISABLE => {
                let except = PrctlArg::PR_THP_DISABLE_EXCEPT_ADVISED.0;
                if arg4 | arg5 != 0 || arg3 & !except != 0 || (arg2 == 0 && arg3 != 0) {
                    return Err(Errno::EINVAL.into());
                }
                state.thp_disable = if arg2 == 0 { 0 } else { 1 | arg3 };
            }
            PrctlOption::PR_FUTEX_HASH => match arg2 {
                1 => {
                    if arg4 != 0 || (arg3 != 0 && (arg3 == 1 || !arg3.is_power_of_two())) {
                        return Err(Errno::EINVAL.into());
                    }
                    if state.futex_slots == Some(0) {
                        return Err(Errno::EBUSY.into());
                    }
                    state.futex_slots = Some(arg3);
                }
                2 => return Ok(state.futex_slots.unwrap_or(0) as i32),
                _ => return Err(Errno::EINVAL.into()),
            },
            _ => unreachable!("only mm prctls are forwarded"),
        }
        Ok(0)
    }
}

/// `PR_SET_SECUREBITS`: locked bits stay, locks stay, and changing anything but the
/// unprivileged bits needs `CAP_SETPCAP`.
fn set_securebits(cred: &Cred, bits: Ulong) -> Result<Cred, Errno> {
    let old = cred.securebits;
    let locked = (old & secure::ALL_LOCKS) >> 1;
    if locked & (old ^ bits) != 0
        || old & secure::ALL_LOCKS & !bits != 0
        || bits & !(secure::ALL_LOCKS | secure::ALL_BITS) != 0
    {
        return Err(Errno::EPERM);
    }
    if !cred.capable(Cap::Setpcap) {
        let unprivileged = secure::ALL_UNPRIVILEGED | secure::ALL_UNPRIVILEGED << 1;
        let changed = old ^ bits;
        // Denying a non-change is Linux's legacy behaviour.
        if changed == 0 || changed & !unprivileged != 0 {
            return Err(Errno::EPERM);
        }
    }
    Ok(Cred {
        securebits: bits,
        ..cred.clone()
    })
}

/// `PR_CAP_AMBIENT`: raising needs the capability in the permitted and inheritable sets.
fn ambient(process: &Process, op: Ulong, cap: Ulong, rest: Ulong) -> SysResult<i32> {
    const IS_SET: Ulong = 1;
    const RAISE: Ulong = 2;
    const LOWER: Ulong = 3;
    const CLEAR_ALL: Ulong = 4;
    if op == CLEAR_ALL {
        if cap | rest != 0 {
            return Err(Errno::EINVAL.into());
        }
        return process.update_cred(|cred| {
            Ok(Cred {
                ambient: 0,
                ..cred.clone()
            })
        });
    }
    if !cap_valid(cap) || rest != 0 {
        return Err(Errno::EINVAL.into());
    }
    let bit = 1 << cap;
    match op {
        IS_SET => Ok((process.cred().ambient & bit != 0) as i32),
        RAISE | LOWER => process.update_cred(|cred| {
            let mut new = cred.clone();
            if op == LOWER {
                new.ambient &= !bit;
                return Ok(new);
            }
            if cred.permitted & cred.inheritable & bit == 0
                || cred.secure(secure::NO_CAP_AMBIENT_RAISE)
            {
                return Err(Errno::EPERM);
            }
            new.ambient |= bit;
            Ok(new)
        }),
        _ => Err(Errno::EINVAL.into()),
    }
}
