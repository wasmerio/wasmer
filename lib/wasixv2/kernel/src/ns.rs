//! Namespaces (`kernel/nsproxy.c`): which mount tree, host names, IPC objects, network, pids,
//! users, cgroups and clocks a task sees. Every task points at an [`NsProxy`], which `clone`
//! shares or copies, `unshare` replaces and `setns` changes, as Linux's `struct nsproxy`.
//!
//! The machine has one namespace of each kind so far, the initial one, with the inode numbers
//! Linux gives those (`linux/nsfs.h`); a request for a new one is refused with `EPERM`, as a
//! container's policy refuses it, until the kind exists. Code reaches every namespaced object
//! through the caller's proxy ([`Kernel::ns`]), never through the kernel, so a kind becomes
//! real by making [`NsProxy::make`] create it.

use std::{
    ops::Deref,
    sync::{Arc, Mutex, Weak},
};

use wasixv2_abi::{CloneFlags, NewUtsname};
use wasixv2_syscalls::{Caller, Errno, TaskId};
use wasixv2_vfs::Cap;

use crate::{Kernel, cred::Cred, ipc, unix};

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
    /// the same inode.
    pub(crate) stashed: Mutex<Weak<crate::nsfs::NsNode>>,
}

/// A namespace: what nsfs knows of it, and the objects it holds.
pub(crate) struct Ns<T> {
    common: Common,
    inner: T,
}

impl<T> Ns<T> {
    /// The initial namespace of `kind`, owned by `owner`.
    fn initial(kind: NsKind, owner: Option<Arc<UserNs>>, inner: T) -> Arc<Self> {
        Arc::new(Self {
            common: Common {
                kind,
                ino: kind.initial_ino(),
                owner,
                stashed: Mutex::default(),
            },
            inner,
        })
    }
}

impl<T> Deref for Ns<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.inner
    }
}

/// A mount namespace. There is one, the VFS's mount tree.
pub(crate) struct Mnt;

/// What a network namespace holds besides its port space: the IP network, without which
/// `socket` fails with `EAFNOSUPPORT`, and the bound Unix sockets.
pub(crate) struct Net {
    pub(crate) network: Option<Arc<dyn wasixv2_vnet::Network>>,
    pub(crate) unix_names: Arc<unix::Names>,
}

/// A pid namespace's place in the hierarchy.
pub(crate) struct Pid {
    parent: Option<Arc<PidNs>>,
}

/// A user namespace's creator.
pub(crate) struct User {
    /// The effective uid that made it (`owner`); root's for the initial one.
    owner: u32,
}

/// A cgroup namespace. There is one, and no cgroup hierarchy behind it.
pub(crate) struct Cgroup;

/// A time namespace. There is one, with no clock offsets.
pub(crate) struct Time;

pub(crate) type MntNs = Ns<Mnt>;
pub(crate) type UtsNs = Ns<Mutex<NewUtsname>>;
pub(crate) type IpcNs = Ns<ipc::Ipc>;
pub(crate) type NetNs = Ns<Net>;
pub(crate) type PidNs = Ns<Pid>;
pub(crate) type UserNs = Ns<User>;
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
                let parent = ns.parent.clone().ok_or(Errno::EPERM)?;
                if !pid_ns_descends(&parent, active_pid) {
                    return Err(Errno::EPERM);
                }
                Ok(Namespace::Pid(parent))
            }
            Namespace::User(_) => self.owner(user).map(Namespace::User),
            _ => Err(Errno::EINVAL),
        }
    }

    /// `NS_GET_OWNER_UID`: the uid that made a user namespace, as the caller sees it.
    pub(crate) fn owner_uid(&self) -> Result<u32, Errno> {
        match self {
            Namespace::User(ns) => Ok(ns.owner),
            _ => Err(Errno::EINVAL),
        }
    }
}

/// `pidns_is_ancestor`: whether `ns` is `ancestor` or below it.
fn pid_ns_descends(ns: &Arc<PidNs>, ancestor: &Arc<PidNs>) -> bool {
    let mut at = Some(ns.clone());
    while let Some(ns) = at {
        if Arc::ptr_eq(&ns, ancestor) {
            return true;
        }
        at = ns.parent.clone();
    }
    false
}

/// The namespaces a task is in (`struct nsproxy`), shared between tasks until one of them gets
/// another. The user namespace, which Linux keeps in the credentials, is here too.
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
    pub(crate) user: Arc<UserNs>,
}

impl NsProxy {
    /// `init_nsproxy`: the initial namespaces, with the UTS names `uname`, the network
    /// `network`, and owned by the initial user namespace, which root made.
    pub(crate) fn initial(
        uname: NewUtsname,
        network: Option<Arc<dyn wasixv2_vnet::Network>>,
    ) -> Self {
        let user = Ns::initial(NsKind::User, None, User { owner: 0 });
        let owner = Some(user.clone());
        let time = Ns::initial(NsKind::Time, owner.clone(), Time);
        Self {
            mnt: Ns::initial(NsKind::Mnt, owner.clone(), Mnt),
            uts: Ns::initial(NsKind::Uts, owner.clone(), Mutex::new(uname)),
            ipc: Ns::initial(NsKind::Ipc, owner.clone(), ipc::Ipc::default()),
            net: Ns::initial(
                NsKind::Net,
                owner.clone(),
                Net {
                    network,
                    unix_names: Arc::default(),
                },
            ),
            pid_for_children: Ns::initial(NsKind::Pid, owner.clone(), Pid { parent: None }),
            cgroup: Ns::initial(NsKind::Cgroup, owner, Cgroup),
            time_for_children: time.clone(),
            time,
            user,
        }
    }

    /// `copy_namespaces` for a process `clone` makes with `flags`, on behalf of a caller with
    /// `cred`: the caller's namespaces, or new ones where `flags` asks (`CAP_SYS_ADMIN`, or a
    /// new user namespace, in which its creator holds every capability). `CLONE_NEWIPC` cannot
    /// go with `CLONE_SYSVSEM`, which would share semaphore adjustments across namespaces. A
    /// child with its own memory starts in the time namespace for children
    /// (`timens_on_fork`).
    pub(crate) fn copy(self: &Arc<Self>, flags: u64, cred: &Cred) -> Result<Arc<Self>, Errno> {
        let vm = flags & CloneFlags::CLONE_VM.0 != 0;
        if flags & CLONE_NS_ALL == 0 {
            if vm || Arc::ptr_eq(&self.time, &self.time_for_children) {
                return Ok(self.clone());
            }
            let mut new = (**self).clone();
            new.time = new.time_for_children.clone();
            return Ok(Arc::new(new));
        }
        let mut new = self.copy_user(flags)?;
        if flags & CLONE_NS_ALL & !CloneFlags::CLONE_NEWUSER.0 != 0 {
            if flags & CloneFlags::CLONE_NEWUSER.0 == 0 && !cred.capable(Cap::SysAdmin) {
                return Err(Errno::EPERM);
            }
            let ipc_sysvsem = CloneFlags::CLONE_NEWIPC.0 | CloneFlags::CLONE_SYSVSEM.0;
            if flags & ipc_sysvsem == ipc_sysvsem {
                return Err(Errno::EINVAL);
            }
            new.create(flags)?;
        }
        if !vm {
            new.time = new.time_for_children.clone();
        }
        Ok(Arc::new(new))
    }

    /// A copy of the proxy, in a new user namespace if `flags` asks (`copy_creds`,
    /// `unshare_userns`).
    fn copy_user(&self, flags: u64) -> Result<Self, Errno> {
        let mut new = self.clone();
        if flags & CloneFlags::CLONE_NEWUSER.0 != 0 {
            new.make(NsKind::User)?;
        }
        Ok(new)
    }

    /// `create_new_namespaces`: a new namespace of each kind `flags` asks for.
    fn create(&mut self, flags: u64) -> Result<(), Errno> {
        for kind in NsKind::PROXIED {
            if flags & kind.flag() != 0 {
                self.make(kind)?;
            }
        }
        Ok(())
    }

    /// Puts a new namespace of `kind` in place of the proxy's (`copy_mnt_ns`, `copy_utsname`,
    /// `copy_ipcs`, `copy_pid_ns`, `copy_cgroup_ns`, `copy_net_ns`, `copy_time_ns`,
    /// `create_user_ns`). No kind can be made yet.
    fn make(&mut self, kind: NsKind) -> Result<(), Errno> {
        match kind {
            NsKind::Mnt
            | NsKind::Uts
            | NsKind::Ipc
            | NsKind::Net
            | NsKind::Pid
            | NsKind::User
            | NsKind::Cgroup
            | NsKind::Time => Err(Errno::EPERM),
        }
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
    /// `pid` (`ns_ops->get`).
    pub(crate) fn get(self, ns: &NsProxy, pid: &Arc<PidNs>) -> Namespace {
        match self {
            NsLink::Net => Namespace::Net(ns.net.clone()),
            NsLink::Uts => Namespace::Uts(ns.uts.clone()),
            NsLink::Ipc => Namespace::Ipc(ns.ipc.clone()),
            NsLink::Pid => Namespace::Pid(pid.clone()),
            NsLink::PidForChildren => Namespace::Pid(ns.pid_for_children.clone()),
            NsLink::User => Namespace::User(ns.user.clone()),
            NsLink::Mnt => Namespace::Mnt(ns.mnt.clone()),
            NsLink::Cgroup => Namespace::Cgroup(ns.cgroup.clone()),
            NsLink::Time => Namespace::Time(ns.time.clone()),
            NsLink::TimeForChildren => Namespace::Time(ns.time_for_children.clone()),
        }
    }
}

impl Kernel {
    /// The namespaces the caller is in. Everything namespaced is reached through them.
    pub(crate) fn ns(&self, caller: &Caller<'_>) -> Arc<NsProxy> {
        self.task_ns(caller.task())
    }

    /// The namespaces `task` is in.
    pub(crate) fn task_ns(&self, task: TaskId) -> Arc<NsProxy> {
        self.task_ref(task, |task| task.ns.clone())
    }
}
