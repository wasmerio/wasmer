//! Namespaces (`kernel/nsproxy.c`): which mount tree, host names, IPC objects, network, pids,
//! users, cgroups and clocks a task sees. Every task points at an [`NsProxy`], which `clone`
//! shares or copies, `unshare` replaces and `setns` changes, as Linux's `struct nsproxy`.
//!
//! The initial namespaces have the inode numbers Linux gives those (`linux/nsfs.h`); those made
//! later get theirs from [`Inums`]. User, mount, UTS, IPC, network and pid namespaces can be
//! made; a request for a new one of a kind that cannot is refused with `EPERM`, as a
//! container's policy refuses it. Every namespace is owned by a user namespace, and acting on
//! it takes the capability over its owner ([`Cred::ns_capable`]). Code reaches every namespaced object through the caller's proxy ([`Kernel::ns`]),
//! never through the kernel, so a kind becomes real by making [`NsProxy::make`] create it.

use std::{
    collections::BTreeSet,
    ops::Deref,
    sync::{Arc, Mutex, Weak},
};

use wasixv2_abi::{CloneFlags, NewUtsname, Ulong};
use wasixv2_syscalls::{Caller, Errno, Fd, SysResult, TaskId};
use wasixv2_vfs::{Cap, Path};

use crate::{
    Kernel, Process, cred::Cred, fs::FsContext, ipc, nsfs::NsFile, pid::PidSpace, sync::MutexExt,
    unix, userns::Ucount,
};

/// `CLONE_NS_ALL`: the flags that ask for new namespaces.
pub(crate) const CLONE_NS_ALL: u64 = CloneFlags::CLONE_NEWCGROUP.0
    | CloneFlags::CLONE_NEWUTS.0
    | CloneFlags::CLONE_NEWIPC.0
    | CloneFlags::CLONE_NEWUSER.0
    | CloneFlags::CLONE_NEWPID.0
    | CloneFlags::CLONE_NEWNET.0
    | CloneFlags::CLONE_NEWNS.0
    | CloneFlags::CLONE_NEWTIME.0;

/// The kinds of namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NsKind {
    Mnt,
    Uts,
    Ipc,
    Net,
    Pid,
    User,
    Cgroup,
    Time,
}

impl NsKind {
    /// The kinds a proxy holds, in `create_new_namespaces`' order; the user namespace comes
    /// before all of them (`copy_creds`).
    const PROXIED: [NsKind; 7] = [
        NsKind::Mnt,
        NsKind::Uts,
        NsKind::Ipc,
        NsKind::Pid,
        NsKind::Cgroup,
        NsKind::Net,
        NsKind::Time,
    ];

    /// The `CLONE_NEW*` flag of the kind, which is also its `ns_type`.
    pub(crate) fn flag(self) -> u64 {
        match self {
            NsKind::Mnt => CloneFlags::CLONE_NEWNS.0,
            NsKind::Uts => CloneFlags::CLONE_NEWUTS.0,
            NsKind::Ipc => CloneFlags::CLONE_NEWIPC.0,
            NsKind::Net => CloneFlags::CLONE_NEWNET.0,
            NsKind::Pid => CloneFlags::CLONE_NEWPID.0,
            NsKind::User => CloneFlags::CLONE_NEWUSER.0,
            NsKind::Cgroup => CloneFlags::CLONE_NEWCGROUP.0,
            NsKind::Time => CloneFlags::CLONE_NEWTIME.0,
        }
    }

    /// The kind's name (`proc_ns_operations.name`), which nsfs shows as `name:[ino]`.
    pub(crate) fn name(self) -> &'static str {
        match self {
            NsKind::Mnt => "mnt",
            NsKind::Uts => "uts",
            NsKind::Ipc => "ipc",
            NsKind::Net => "net",
            NsKind::Pid => "pid",
            NsKind::User => "user",
            NsKind::Cgroup => "cgroup",
            NsKind::Time => "time",
        }
    }

    /// The inode number of the initial namespace of the kind (`enum init_ns_ino`).
    fn initial_ino(self) -> u32 {
        match self {
            NsKind::Ipc => 0xEFFF_FFFF,
            NsKind::Uts => 0xEFFF_FFFE,
            NsKind::User => 0xEFFF_FFFD,
            NsKind::Pid => 0xEFFF_FFFC,
            NsKind::Cgroup => 0xEFFF_FFFB,
            NsKind::Time => 0xEFFF_FFFA,
            NsKind::Net => 0xEFFF_FFF9,
            NsKind::Mnt => 0xEFFF_FFF8,
        }
    }
}

/// What every namespace has (`struct ns_common`).
pub(crate) struct Common {
    kind: NsKind,
    /// The inode number nsfs gives it (`inum`).
    ino: u32,
    /// The user namespace that owns it (`ns->ops->owner`): for a user namespace its parent,
    /// which the initial one does not have.
    owner: Option<Arc<UserNs>>,
    /// Its nsfs inode while one is in use (`stashed`), so every open of the namespace finds
    /// the same inode. Held as the VFS sees it: a namespace's own type does not reach back to
    /// every other namespace through the inode's.
    pub(crate) stashed: Mutex<Weak<dyn wasixv2_vfs::Node>>,
    /// Where `ino` goes back when the namespace is gone; none for an initial namespace.
    inums: Option<Arc<Inums>>,
    /// The namespace's count against its creator's `max_*_namespaces`; none for an initial
    /// namespace.
    _ucount: Option<Ucount>,
}

impl Drop for Common {
    /// `ns_common_free`: the inode number is free again.
    fn drop(&mut self) {
        if let Some(inums) = &self.inums {
            inums.free(self.ino);
        }
    }
}

/// `PROC_DYNAMIC_FIRST`: the first inode number of a namespace made after boot.
const DYNAMIC_FIRST: u32 = 0xF000_0000;

/// `proc_alloc_inum`: the inode numbers of the namespaces made after boot, from
/// [`DYNAMIC_FIRST`] up, the lowest free one first (`proc_inum_ida`).
#[derive(Default)]
pub(crate) struct Inums {
    state: Mutex<InumState>,
}

#[derive(Default)]
struct InumState {
    /// Offsets from [`DYNAMIC_FIRST`]: every one below `next` is in use unless in `free`.
    next: u32,
    free: BTreeSet<u32>,
}

impl Inums {
    /// A free inode number; `ENOSPC` when none is left below `u32::MAX`.
    fn alloc(&self) -> Result<u32, Errno> {
        let mut state = self.state.lock_or_recover();
        let offset = match state.free.pop_first() {
            Some(offset) => offset,
            None if state.next <= u32::MAX - DYNAMIC_FIRST => {
                state.next += 1;
                state.next - 1
            }
            None => return Err(Errno::ENOSPC),
        };
        Ok(DYNAMIC_FIRST + offset)
    }

    fn free(&self, ino: u32) {
        self.state
            .lock_or_recover()
            .free
            .insert(ino - DYNAMIC_FIRST);
    }
}

/// A namespace: what nsfs knows of it, and the objects it holds.
pub(crate) struct Ns<T> {
    common: Common,
    inner: T,
}

impl<T> Ns<T> {
    /// The initial namespace of `kind`, owned by `owner`.
    pub(crate) fn initial(kind: NsKind, owner: Option<Arc<UserNs>>, inner: T) -> Arc<Self> {
        Arc::new(Self {
            common: Common {
                kind,
                ino: kind.initial_ino(),
                owner,
                stashed: Mutex::new(Weak::<crate::nsfs::NsNode>::new()),
                inums: None,
                _ucount: None,
            },
            inner,
        })
    }

    /// A namespace of `kind` made after boot (`ns_common_init`), owned by `owner`, with an
    /// inode number from `inums`, counted as `ucount`.
    pub(crate) fn new(
        kind: NsKind,
        owner: Arc<UserNs>,
        inner: T,
        inums: &Arc<Inums>,
        ucount: Ucount,
    ) -> Result<Arc<Self>, Errno> {
        Ok(Arc::new(Self {
            common: Common {
                kind,
                ino: inums.alloc()?,
                owner: Some(owner),
                stashed: Mutex::new(Weak::<crate::nsfs::NsNode>::new()),
                inums: Some(inums.clone()),
                _ucount: Some(ucount),
            },
            inner,
        }))
    }
}

impl<T> Ns<T> {
    /// The user namespace that owns this one (`ns->ops->owner`); for a user namespace, its
    /// parent.
    pub(crate) fn owner_ns(&self) -> Option<&Arc<UserNs>> {
        self.common.owner.as_ref()
    }

    /// The user namespace that owns a namespace of a kind other than user (`ns->user_ns`),
    /// over which acting on it takes capabilities.
    pub(crate) fn user_ns(&self) -> &Arc<UserNs> {
        self.owner_ns()
            .expect("only the initial user namespace has no owner")
    }
}

impl<T> Deref for Ns<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.inner
    }
}

/// A mount namespace: a tree of the VFS's mounts.
pub(crate) struct Mnt {
    pub(crate) tree: Arc<wasixv2_vfs::MntNs>,
}

/// Makes the IP network of a network namespace made after boot.
pub(crate) type NetworkFactory = Arc<dyn Fn() -> Arc<dyn wasixv2_vnet::Network> + Send + Sync>;

/// What a network namespace holds besides its port space: the IP network, without which
/// `socket` fails with `EAFNOSUPPORT`, the bound Unix sockets, and IPv4's configuration of
/// its interfaces.
pub(crate) struct Net {
    pub(crate) network: Option<Arc<dyn wasixv2_vnet::Network>>,
    pub(crate) unix_names: Arc<unix::Names>,
    pub(crate) devconf: Mutex<crate::devinet::Devconf>,
}

/// A cgroup namespace. There is one, and no cgroup hierarchy behind it.
pub(crate) struct Cgroup;

/// A time namespace. There is one, with no clock offsets.
pub(crate) struct Time;

pub(crate) type MntNs = Ns<Mnt>;
pub(crate) type UtsNs = Ns<Mutex<NewUtsname>>;
pub(crate) type IpcNs = Ns<ipc::Ipc>;
pub(crate) type NetNs = Ns<Net>;
pub(crate) type PidNs = Ns<PidSpace>;
pub(crate) type UserNs = Ns<crate::userns::User>;
pub(crate) type CgroupNs = Ns<Cgroup>;
pub(crate) type TimeNs = Ns<Time>;

/// Any namespace, as an nsfs inode stands for one.
#[derive(Clone)]
pub(crate) enum Namespace {
    Mnt(Arc<MntNs>),
    Uts(Arc<UtsNs>),
    Ipc(Arc<IpcNs>),
    Net(Arc<NetNs>),
    Pid(Arc<PidNs>),
    User(Arc<UserNs>),
    Cgroup(Arc<CgroupNs>),
    Time(Arc<TimeNs>),
}

impl Namespace {
    pub(crate) fn common(&self) -> &Common {
        match self {
            Namespace::Mnt(ns) => &ns.common,
            Namespace::Uts(ns) => &ns.common,
            Namespace::Ipc(ns) => &ns.common,
            Namespace::Net(ns) => &ns.common,
            Namespace::Pid(ns) => &ns.common,
            Namespace::User(ns) => &ns.common,
            Namespace::Cgroup(ns) => &ns.common,
            Namespace::Time(ns) => &ns.common,
        }
    }

    pub(crate) fn kind(&self) -> NsKind {
        self.common().kind
    }

    pub(crate) fn ino(&self) -> u32 {
        self.common().ino
    }

    /// `ns_get_owner`: the user namespace that owns this one, if the caller, in the user
    /// namespace `mine`, can see it: it must be `mine` or one of its ancestors (`EPERM`).
    pub(crate) fn owner(&self, mine: &Arc<UserNs>) -> Result<Arc<UserNs>, Errno> {
        let owner = self.common().owner.clone().ok_or(Errno::EPERM)?;
        let mut ancestor = Some(mine.clone());
        while let Some(ns) = ancestor {
            if Arc::ptr_eq(&ns, &owner) {
                return Ok(owner);
            }
            ancestor = ns.common.owner.clone();
        }
        Err(Errno::EPERM)
    }

    /// `ns->ops->get_parent`, for `NS_GET_PARENT`: the parent of a pid namespace if it is the
    /// caller's active one (`active_pid`) or a descendant of it, of a user namespace if it is
    /// `user` or an ancestor (`EPERM` otherwise, and for an initial namespace). Other kinds
    /// have no hierarchy (`EINVAL`).
    pub(crate) fn parent(
        &self,
        user: &Arc<UserNs>,
        active_pid: &Arc<PidNs>,
    ) -> Result<Namespace, Errno> {
        match self {
            Namespace::Pid(ns) => {
                let parent = ns.parent().cloned().ok_or(Errno::EPERM)?;
                if !pid_ns_descends(&parent, active_pid) {
                    return Err(Errno::EPERM);
                }
                Ok(Namespace::Pid(parent))
            }
            Namespace::User(_) => self.owner(user).map(Namespace::User),
            _ => Err(Errno::EINVAL),
        }
    }

    /// `NS_GET_OWNER_UID`: the uid that made a user namespace, as the caller, in the user
    /// namespace `mine`, sees it.
    pub(crate) fn owner_uid(&self, mine: &UserNs) -> Result<u32, Errno> {
        match self {
            Namespace::User(ns) => Ok(mine.munged_uid(ns.owner())),
            _ => Err(Errno::EINVAL),
        }
    }
}

/// `pidns_is_ancestor`: whether `ns` is `ancestor` or below it.
pub(crate) fn pid_ns_descends(ns: &Arc<PidNs>, ancestor: &Arc<PidNs>) -> bool {
    let mut at = Some(ns.clone());
    while let Some(ns) = at {
        if Arc::ptr_eq(&ns, ancestor) {
            return true;
        }
        at = ns.parent().cloned();
    }
    false
}

/// The namespaces a task is in (`struct nsproxy`), shared between tasks until one of them gets
/// another. The user namespace is in the process's credentials ([`Cred::user_ns`]), as on
/// Linux.
#[derive(Clone)]
pub(crate) struct NsProxy {
    pub(crate) mnt: Arc<MntNs>,
    pub(crate) uts: Arc<UtsNs>,
    pub(crate) ipc: Arc<IpcNs>,
    pub(crate) net: Arc<NetNs>,
    /// The pid namespace the task's children start in; the task's own is its process's
    /// ([`Process::pid_ns`]).
    pub(crate) pid_for_children: Arc<PidNs>,
    pub(crate) cgroup: Arc<CgroupNs>,
    pub(crate) time: Arc<TimeNs>,
    /// The time namespace the task's children start in.
    pub(crate) time_for_children: Arc<TimeNs>,
}

impl NsProxy {
    /// `init_nsproxy`: the initial namespaces, with the mount tree `mnt`, the UTS names
    /// `uname`, the network `network`, and owned by the initial user namespace `user`.
    pub(crate) fn initial(
        user: &Arc<UserNs>,
        mnt: Arc<wasixv2_vfs::MntNs>,
        uname: NewUtsname,
        network: Option<Arc<dyn wasixv2_vnet::Network>>,
    ) -> Self {
        let owner = Some(user.clone());
        let time = Ns::initial(NsKind::Time, owner.clone(), Time);
        Self {
            mnt: Ns::initial(NsKind::Mnt, owner.clone(), Mnt { tree: mnt }),
            uts: Ns::initial(NsKind::Uts, owner.clone(), Mutex::new(uname)),
            ipc: Ns::initial(NsKind::Ipc, owner.clone(), ipc::Ipc::new((0, 0))),
            net: Ns::initial(
                NsKind::Net,
                owner.clone(),
                Net {
                    network,
                    unix_names: Arc::default(),
                    devconf: Mutex::default(),
                },
            ),
            pid_for_children: Ns::initial(NsKind::Pid, owner.clone(), PidSpace::initial()),
            cgroup: Ns::initial(NsKind::Cgroup, owner, Cgroup),
            time_for_children: time.clone(),
            time,
        }
    }

    /// `copy_namespaces` for a process `clone` makes with `flags`, with the child's credentials
    /// `cred` (in a new user namespace for `CLONE_NEWUSER`, made before), on behalf of a caller
    /// in the pid namespace `active_pid`: the caller's namespaces, or new ones where `flags`
    /// asks, owned by `cred`'s user namespace (`CAP_SYS_ADMIN` there, which the creator of a
    /// new user namespace holds). `CLONE_NEWIPC` cannot go with `CLONE_SYSVSEM`, which would
    /// share semaphore adjustments across namespaces. A child with its own memory starts in
    /// the time namespace for children (`timens_on_fork`).
    pub(crate) fn copy(
        self: &Arc<Self>,
        kernel: &Kernel,
        flags: u64,
        cred: &Cred,
        active_pid: &Arc<PidNs>,
        fs: Option<&mut FsContext>,
    ) -> Result<Arc<Self>, Errno> {
        let vm = flags & CloneFlags::CLONE_VM.0 != 0;
        if flags & CLONE_NS_ALL == 0 {
            if vm || Arc::ptr_eq(&self.time, &self.time_for_children) {
                return Ok(self.clone());
            }
            let mut new = (**self).clone();
            new.time = new.time_for_children.clone();
            return Ok(Arc::new(new));
        }
        let mut new = (**self).clone();
        if flags & CLONE_NS_ALL & !CloneFlags::CLONE_NEWUSER.0 != 0 {
            if !cred.capable(Cap::SysAdmin) {
                return Err(Errno::EPERM);
            }
            let ipc_sysvsem = CloneFlags::CLONE_NEWIPC.0 | CloneFlags::CLONE_SYSVSEM.0;
            if flags & ipc_sysvsem == ipc_sysvsem {
                return Err(Errno::EINVAL);
            }
            new.create(kernel, flags, cred, active_pid, fs)?;
        }
        if !vm {
            new.time = new.time_for_children.clone();
        }
        Ok(Arc::new(new))
    }

    /// `unshare_nsproxy_namespaces`: the namespaces of a caller in the pid namespace
    /// `active_pid` with new ones where `flags` asks, owned by the user namespace of its
    /// credentials `cred` (new ones for `CLONE_NEWUSER`, made before), or `None` if it asks for
    /// none but a user namespace.
    pub(crate) fn unshare(
        &self,
        kernel: &Kernel,
        flags: u64,
        cred: &Cred,
        active_pid: &Arc<PidNs>,
        fs: Option<&mut FsContext>,
    ) -> Result<Option<Self>, Errno> {
        if flags & CLONE_NS_ALL & !CloneFlags::CLONE_NEWUSER.0 == 0 {
            return Ok(None);
        }
        if !cred.capable(Cap::SysAdmin) {
            return Err(Errno::EPERM);
        }
        let mut new = self.clone();
        new.create(kernel, flags, cred, active_pid, fs)?;
        Ok(Some(new))
    }

    /// `create_new_namespaces`: a new namespace of each kind `flags` asks for.
    fn create(
        &mut self,
        kernel: &Kernel,
        flags: u64,
        cred: &Cred,
        active_pid: &Arc<PidNs>,
        mut fs: Option<&mut FsContext>,
    ) -> Result<(), Errno> {
        for kind in NsKind::PROXIED {
            if flags & kind.flag() != 0 {
                self.make(kernel, kind, cred, active_pid, fs.as_deref_mut())?;
            }
        }
        Ok(())
    }

    /// Puts a new namespace of `kind` in place of the proxy's (`copy_mnt_ns`, `copy_utsname`,
    /// `copy_ipcs`, `copy_pid_ns`, `copy_cgroup_ns`, `copy_net_ns`, `copy_time_ns`,
    /// owned by the user namespace of the maker's credentials `cred`. A new mount namespace copies
    /// the mount tree and moves `fs`, the root and working directory, to the copies of their
    /// mounts (`copy_mnt_ns`). A new UTS namespace starts
    /// with a copy of the names (`clone_uts_ns`), a new IPC namespace empty (`create_ipc_ns`),
    /// a new network namespace with the network the kernel's factory makes and no abstract Unix
    /// names (`setup_net`). A new pid namespace goes below the one for children, which must be
    /// the maker's own, `active_pid` (`copy_pid_ns`), and is for the children only. Cgroup
    /// and time namespaces cannot be made yet.
    fn make(
        &mut self,
        kernel: &Kernel,
        kind: NsKind,
        cred: &Cred,
        active_pid: &Arc<PidNs>,
        fs: Option<&mut FsContext>,
    ) -> Result<(), Errno> {
        let inums = &kernel.ns_inums;
        let owner = cred.user_ns.clone();
        // `inc_*_namespaces`: counted for the creator's effective uid.
        let charge = || Ucount::charge(&owner, cred.user.effective, kind);
        match kind {
            NsKind::Uts => {
                let ucount = charge()?;
                let names = *self.uts.lock_or_recover();
                self.uts = Ns::new(kind, owner, Mutex::new(names), inums, ucount)?;
            }
            NsKind::Ipc => {
                let ucount = charge()?;
                let ipc = ipc::Ipc::new(cred.fs_owner());
                self.ipc = Ns::new(kind, owner, ipc, inums, ucount)?;
            }
            NsKind::Net => {
                let ucount = charge()?;
                let initial = kernel.init_ns.net.devconf.lock_or_recover().for_new_net();
                let net = Net {
                    network: kernel.network_factory.as_ref().map(|make| make()),
                    unix_names: Arc::new(self.net.unix_names.for_new_net()),
                    devconf: Mutex::new(initial),
                };
                self.net = Ns::new(kind, owner, net, inums, ucount)?;
            }
            NsKind::Mnt => {
                let ucount = charge()?;
                let tree_owner: Arc<dyn wasixv2_vfs::UserNs> = owner.clone();
                let tree = match fs {
                    Some(fs) => self
                        .mnt
                        .tree
                        .copy(tree_owner, &mut [&mut fs.root, &mut fs.cwd]),
                    None => self.mnt.tree.copy(tree_owner, &mut []),
                };
                self.mnt = Ns::new(kind, owner, Mnt { tree }, inums, ucount)?;
            }
            NsKind::Pid => {
                let parent = &self.pid_for_children;
                if !Arc::ptr_eq(active_pid, parent) {
                    return Err(Errno::EINVAL);
                }
                let space = PidSpace::child_of(parent)?;
                let ucount = charge()?;
                self.pid_for_children = Ns::new(kind, owner, space, inums, ucount)?;
            }
            // A user namespace is made with the credentials, before the others
            // (`UserNs::create`).
            NsKind::User | NsKind::Cgroup | NsKind::Time => {
                return Err(Errno::EPERM);
            }
        }
        Ok(())
    }
}

/// The links of `/proc/<pid>/ns` (`ns_entries`), in Linux's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NsLink {
    Net,
    Uts,
    Ipc,
    Pid,
    PidForChildren,
    User,
    Mnt,
    Cgroup,
    Time,
    TimeForChildren,
}

impl NsLink {
    pub(crate) const ALL: [NsLink; 10] = [
        NsLink::Net,
        NsLink::Uts,
        NsLink::Ipc,
        NsLink::Pid,
        NsLink::PidForChildren,
        NsLink::User,
        NsLink::Mnt,
        NsLink::Cgroup,
        NsLink::Time,
        NsLink::TimeForChildren,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            NsLink::Net => "net",
            NsLink::Uts => "uts",
            NsLink::Ipc => "ipc",
            NsLink::Pid => "pid",
            NsLink::PidForChildren => "pid_for_children",
            NsLink::User => "user",
            NsLink::Mnt => "mnt",
            NsLink::Cgroup => "cgroup",
            NsLink::Time => "time",
            NsLink::TimeForChildren => "time_for_children",
        }
    }

    /// The namespace the link shows of a task in `ns`, whose process is in the pid namespace
    /// `pid` and the user namespace `user` (`ns_ops->get`).
    pub(crate) fn get(self, ns: &NsProxy, pid: &Arc<PidNs>, user: &Arc<UserNs>) -> Namespace {
        match self {
            NsLink::Net => Namespace::Net(ns.net.clone()),
            NsLink::Uts => Namespace::Uts(ns.uts.clone()),
            NsLink::Ipc => Namespace::Ipc(ns.ipc.clone()),
            NsLink::Pid => Namespace::Pid(pid.clone()),
            NsLink::PidForChildren => Namespace::Pid(ns.pid_for_children.clone()),
            NsLink::User => Namespace::User(user.clone()),
            NsLink::Mnt => Namespace::Mnt(ns.mnt.clone()),
            NsLink::Cgroup => Namespace::Cgroup(ns.cgroup.clone()),
            NsLink::Time => Namespace::Time(ns.time.clone()),
            NsLink::TimeForChildren => Namespace::Time(ns.time_for_children.clone()),
        }
    }
}

/// `struct nsset`: the caller's namespaces as `setns` changes them, before it commits.
struct Nsset {
    flags: u64,
    proxy: NsProxy,
    /// The root and working directory entering a mount namespace gives.
    root: Option<Path>,
    /// The credentials entering a user namespace gives (`nsset->cred`).
    cred: Option<Cred>,
}

impl Nsset {
    /// The user namespace the caller ends up in (`nsset->cred->user_ns`).
    fn user_ns<'a>(&'a self, joiner: &'a Joiner<'_>) -> &'a Arc<UserNs> {
        &self.cred.as_ref().unwrap_or(&joiner.cred).user_ns
    }

    /// The `*ns_install` check for a namespace owned by `owner`: the caller has
    /// `CAP_SYS_ADMIN` over it and over the user namespace it ends up in.
    fn may_enter(&self, joiner: &Joiner<'_>, owner: &Arc<UserNs>) -> bool {
        joiner.cred.ns_capable(owner, Cap::SysAdmin)
            && joiner.cred.ns_capable(self.user_ns(joiner), Cap::SysAdmin)
    }
}

/// Who calls `setns`.
struct Joiner<'a> {
    process: &'a Process,
    cred: Cred,
    /// No other task runs in the caller's thread group (`thread_group_empty`).
    alone: bool,
    /// Nor on its memory (`current_is_single_threaded`).
    single: bool,
    /// Another task shares the caller's root and working directory (`fs->users != 1`).
    fs_shared: bool,
}

impl Kernel {
    /// The namespaces the caller is in. Everything namespaced is reached through them.
    pub(crate) fn ns(&self, caller: &Caller<'_>) -> Arc<NsProxy> {
        self.task_ns(caller.task())
    }

    /// The namespaces `task` is in.
    pub(crate) fn task_ns(&self, task: TaskId) -> Arc<NsProxy> {
        self.task_ref(task, |task| task.live_ns().clone())
    }

    /// `exit_sem` for a process that changes IPC namespace or stops sharing its semaphore
    /// adjustments: they apply now, unless another of its tasks still shares them
    /// (`sem_undo_list.refcnt`).
    fn exit_sem(&self, process: &Process) {
        if process.group.single_threaded()
            && let Some(ipc) = process.sysvsem.lock_or_recover().clone()
        {
            ipc.sem.exit(&self.pids.get(process.pid.0), self.ipc_now());
        }
    }

    /// `ksys_unshare`: the caller stops sharing what `flags` names with other tasks: its
    /// descriptor table (`CLONE_FILES`), root and working directory (`CLONE_FS`), semaphore
    /// adjustments (`CLONE_SYSVSEM`), and namespaces (`CLONE_NEW*`). Unsharing what only a
    /// thread group shares (`CLONE_THREAD`, `CLONE_SIGHAND`, `CLONE_VM`) is not implemented,
    /// as on Linux, and works only when nothing shares it (`check_unshare_flags`).
    pub(crate) fn sys_unshare(&self, caller: &Caller<'_>, flags: Ulong) -> SysResult<i32> {
        let mut flags = u64::from(flags);
        if flags & CloneFlags::CLONE_NEWUSER.0 != 0 {
            flags |= CloneFlags::CLONE_THREAD.0 | CloneFlags::CLONE_FS.0;
        }
        if flags & CloneFlags::CLONE_VM.0 != 0 {
            flags |= CloneFlags::CLONE_SIGHAND.0;
        }
        if flags & CloneFlags::CLONE_SIGHAND.0 != 0 {
            flags |= CloneFlags::CLONE_THREAD.0;
        }
        if flags & CloneFlags::CLONE_NEWNS.0 != 0 {
            flags |= CloneFlags::CLONE_FS.0;
        }
        let known = CloneFlags::CLONE_THREAD.0
            | CloneFlags::CLONE_FS.0
            | CloneFlags::CLONE_SIGHAND.0
            | CloneFlags::CLONE_VM.0
            | CloneFlags::CLONE_FILES.0
            | CloneFlags::CLONE_SYSVSEM.0
            | CLONE_NS_ALL;
        if flags & !known != 0 {
            return Err(Errno::EINVAL.into());
        }
        let task = caller.task();
        let (process, files, fs, ns) = self.task_ref(task, |t| {
            (
                t.process.clone(),
                t.files.clone(),
                t.fs.clone(),
                t.live_ns().clone(),
            )
        });
        let thread_group =
            CloneFlags::CLONE_THREAD.0 | CloneFlags::CLONE_SIGHAND.0 | CloneFlags::CLONE_VM.0;
        // Handlers are shared only within a thread group here, so the first check also covers
        // `sighand->count`.
        if flags & thread_group != 0 && !process.group.single_threaded() {
            return Err(Errno::EINVAL.into());
        }
        if flags & CloneFlags::CLONE_VM.0 != 0 && self.mm_shared_elsewhere(task) {
            return Err(Errno::EINVAL.into());
        }
        // The task's reference and ours: anything above is another task's.
        let shared = |count: usize| count > 2;
        // A new mount namespace moves the root and working directory, so the caller gets its
        // own copy of them whether or not another task shares them.
        let mut new_fs = (flags & CloneFlags::CLONE_FS.0 != 0
            && (flags & CloneFlags::CLONE_NEWNS.0 != 0 || shared(Arc::strong_count(&fs))))
        .then(|| fs.lock_or_recover().clone());
        let new_files =
            match flags & CloneFlags::CLONE_FILES.0 != 0 && shared(Arc::strong_count(&files)) {
                true => Some(Arc::new(Mutex::new(files.lock_or_recover().dup()?))),
                false => None,
            };
        // `unshare_userns`: new credentials in a new user namespace, which owns the other new
        // namespaces.
        let new_cred = match flags & CloneFlags::CLONE_NEWUSER.0 != 0 {
            true => {
                let chrooted = fs.lock_or_recover().chrooted(&ns);
                Some(UserNs::create(&process.cred(), chrooted, &self.ns_inums)?)
            }
            false => None,
        };
        let cred = new_cred.clone().unwrap_or_else(|| process.cred());
        let new_ns = ns.unshare(self, flags, &cred, &process.pid_ns, new_fs.as_mut())?;
        if let Some(cred) = new_cred {
            process.commit_cred(cred);
        }
        let ipc_or_sysvsem = CloneFlags::CLONE_NEWIPC.0 | CloneFlags::CLONE_SYSVSEM.0;
        if flags & ipc_or_sysvsem != 0 {
            self.exit_sem(&process);
        }
        if let Some(new) = &new_ns
            && flags & CloneFlags::CLONE_NEWIPC.0 != 0
        {
            *process.sysvsem.lock_or_recover() = Some(new.ipc.clone());
        }
        self.with_task(task, |t| {
            if let Some(fs) = new_fs {
                t.fs = Arc::new(Mutex::new(fs));
            }
            if let Some(ns) = new_ns {
                t.ns = Some(Arc::new(ns));
            }
            if let Some(files) = new_files {
                t.files = files;
            }
        });
        Ok(0)
    }

    /// `setns`: the caller enters the namespace an nsfs descriptor stands for (`flags`, if not
    /// 0, must be its kind), or those of the kinds `flags` names that the process a pidfd
    /// refers to is in. Every namespace is checked before any is entered.
    pub(crate) fn sys_setns(&self, caller: &Caller<'_>, fd: Fd, flags: i32) -> SysResult<i32> {
        let file = self.io_file(caller, fd)?;
        let flags = u64::from(flags as u32);
        enum Target {
            One(Namespace),
            Process(u32),
        }
        let (flags, target) = if let Some(ns) = file.downcast::<NsFile>() {
            let kind = ns.namespace().kind().flag();
            if flags != 0 && flags != kind {
                return Err(Errno::EINVAL.into());
            }
            (kind, Target::One(ns.namespace().clone()))
        } else if let Some(pid) = crate::process::pidfd_target(&file) {
            // `check_setns_flags`.
            if flags == 0 || flags & !CLONE_NS_ALL != 0 {
                return Err(Errno::EINVAL.into());
            }
            (flags, Target::Process(pid))
        } else {
            return Err(Errno::EINVAL.into());
        };
        let task = caller.task();
        let (process, fs, ns) = self.task_ref(task, |t| {
            (t.process.clone(), t.fs.clone(), t.live_ns().clone())
        });
        let joiner = Joiner {
            process: &process,
            cred: process.cred(),
            alone: process.group.single_threaded(),
            single: process.group.single_threaded() && !self.mm_shared_elsewhere(task),
            fs_shared: Arc::strong_count(&fs) > 2,
        };
        let mut set = Nsset {
            flags,
            proxy: (*ns).clone(),
            root: None,
            cred: None,
        };
        match target {
            Target::One(ns) => self.install_ns(&mut set, &joiner, ns)?,
            Target::Process(pid) => self.install_ns_of(&mut set, &joiner, pid)?,
        }
        // `commit_nsset`: the credentials first.
        if let Some(cred) = set.cred.take() {
            process.commit_cred(cred);
        }
        if let Some(root) = set.root.take() {
            let mut fs = fs.lock_or_recover();
            fs.cwd = root.clone();
            fs.root = root;
        }
        if flags & CloneFlags::CLONE_NEWIPC.0 != 0 {
            self.exit_sem(&process);
            *process.sysvsem.lock_or_recover() = Some(set.proxy.ipc.clone());
        }
        let proxy = Arc::new(set.proxy);
        self.with_task(task, |t| t.ns = Some(proxy));
        Ok(0)
    }

    /// `validate_nsset`: installs the namespaces of the kinds `set.flags` names that process
    /// `pid` is in, user namespace first. The process must be alive and the caller allowed to
    /// read its state (`PTRACE_MODE_READ_REALCREDS`).
    fn install_ns_of(&self, set: &mut Nsset, joiner: &Joiner<'_>, pid: u32) -> Result<(), Errno> {
        let target = {
            let state = self.state.read_or_recover();
            state
                .tasks
                .get(&TaskId(pid))
                .filter(|t| t.process.pid.0 == pid && !t.thread.exited())
                .and_then(|t| Some((t.process.clone(), t.ns.clone()?)))
        };
        let (process, ns) = target.ok_or(Errno::ESRCH)?;
        let dumpable = process.ident.lock_or_recover().dumpable;
        if !std::ptr::eq(&*process, joiner.process)
            && !joiner.cred.may_read_task(&process.cred(), dumpable, false)
        {
            return Err(Errno::EPERM);
        }
        let order = [
            NsKind::User,
            NsKind::Mnt,
            NsKind::Uts,
            NsKind::Ipc,
            NsKind::Pid,
            NsKind::Cgroup,
            NsKind::Net,
            NsKind::Time,
        ];
        for kind in order {
            if set.flags & kind.flag() == 0 {
                continue;
            }
            let theirs = match kind {
                NsKind::Mnt => Namespace::Mnt(ns.mnt.clone()),
                NsKind::Uts => Namespace::Uts(ns.uts.clone()),
                NsKind::Ipc => Namespace::Ipc(ns.ipc.clone()),
                NsKind::Net => Namespace::Net(ns.net.clone()),
                NsKind::Pid => Namespace::Pid(process.pid_ns.clone()),
                NsKind::User => Namespace::User(process.cred().user_ns),
                NsKind::Cgroup => Namespace::Cgroup(ns.cgroup.clone()),
                NsKind::Time => Namespace::Time(ns.time.clone()),
            };
            self.install_ns(set, joiner, theirs)?;
        }
        Ok(())
    }

    /// `ns->ops->install`: puts `ns` in `set`, if the caller may enter it: with
    /// `CAP_SYS_ADMIN` over the user namespace that owns it, and over the one it ends up in
    /// ([`Nsset::may_enter`]); a user namespace itself with `CAP_SYS_ADMIN` over it.
    fn install_ns(&self, set: &mut Nsset, joiner: &Joiner<'_>, ns: Namespace) -> Result<(), Errno> {
        let owner = ns.common().owner.clone();
        let may_enter = |set: &Nsset| {
            let owner = owner.as_ref().expect("only a user namespace has no owner");
            set.may_enter(joiner, owner)
        };
        match ns {
            // `userns_install`: entering one's own user namespace would regain capabilities,
            // and a thread group shares one.
            Namespace::User(ns) => {
                if Arc::ptr_eq(&ns, &joiner.cred.user_ns) || !joiner.alone || joiner.fs_shared {
                    return Err(Errno::EINVAL);
                }
                if !joiner.cred.ns_capable(&ns, Cap::SysAdmin) {
                    return Err(Errno::EPERM);
                }
                let cred = set.cred.as_ref().unwrap_or(&joiner.cred);
                set.cred = Some(cred.for_user_ns(ns));
            }
            // `mntns_install`: the root and working directory become the namespace's root. Only
            // a caller entering nothing else checks its own `fs_struct`; otherwise Linux checks
            // a private copy.
            Namespace::Mnt(ns) => {
                let user_ns = set.user_ns(joiner);
                if !joiner
                    .cred
                    .ns_capable(owner.as_ref().expect("owned"), Cap::SysAdmin)
                    || !joiner.cred.ns_capable(user_ns, Cap::SysChroot)
                    || !joiner.cred.ns_capable(user_ns, Cap::SysAdmin)
                {
                    return Err(Errno::EPERM);
                }
                if set.flags == CloneFlags::CLONE_NEWNS.0 && joiner.fs_shared {
                    return Err(Errno::EINVAL);
                }
                set.root = Some(ns.tree.entry_root());
                set.proxy.mnt = ns;
            }
            Namespace::Uts(ns) => {
                if !may_enter(set) {
                    return Err(Errno::EPERM);
                }
                set.proxy.uts = ns;
            }
            Namespace::Ipc(ns) => {
                if !may_enter(set) {
                    return Err(Errno::EPERM);
                }
                set.proxy.ipc = ns;
            }
            Namespace::Net(ns) => {
                if !may_enter(set) {
                    return Err(Errno::EPERM);
                }
                set.proxy.net = ns;
            }
            // `pidns_install`: only the caller's active pid namespace or one below it, so its
            // children stay visible to it.
            Namespace::Pid(ns) => {
                if !may_enter(set) {
                    return Err(Errno::EPERM);
                }
                if !pid_ns_descends(&ns, &joiner.process.pid_ns) {
                    return Err(Errno::EINVAL);
                }
                set.proxy.pid_for_children = ns;
            }
            Namespace::Cgroup(ns) => {
                if !may_enter(set) {
                    return Err(Errno::EPERM);
                }
                set.proxy.cgroup = ns;
            }
            // `timens_install`: the clocks change under every task of the memory.
            Namespace::Time(ns) => {
                if !joiner.single {
                    return Err(Errno::EUSERS);
                }
                if !may_enter(set) {
                    return Err(Errno::EPERM);
                }
                set.proxy.time = ns.clone();
                set.proxy.time_for_children = ns;
            }
        }
        Ok(())
    }
}
