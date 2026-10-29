//! System V IPC and POSIX message queues (`ipc/`): semaphore sets, message queues, shared memory
//! segments and `mqueue` queues.
//!
//! Semaphores and both kinds of message queue live entirely in the kernel, so they work between
//! any processes of the machine exactly as on Linux. Shared memory segments are kernel buffers
//! too, but an attachment is a copy in the attaching process's linear memory, kept coherent at
//! defined points; see [`shm`].
//!
//! Identifiers, keys and permissions follow `ipc/util.c`: an id is `seq * 32768 + index`, keys
//! other than `IPC_PRIVATE` name at most one object, and access is checked against the owner,
//! group and other bits of `ipc_perm` with `CAP_IPC_OWNER` as the override.

pub(crate) mod mqueue;
pub(crate) mod msg;
pub(crate) mod sem;
pub(crate) mod shm;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};

use wasixv2_abi::{Ipc64Perm, IpcCmd, IpcFlags, IpcKey};
use wasixv2_syscalls::Caller;
use wasixv2_syscalls::Errno;
use wasixv2_vfs::Cap;

use crate::{
    Kernel,
    cred::Cred as TaskCred,
    ns::UserNs,
    sync::MutexExt,
    uidgid::{Kgid, Kuid},
};

/// `IPCMNI`: indices of one kind of object stay below it.
const IPCMNI: usize = 32768;
/// `ipc_min_cycle` (`RADIX_TREE_MAP_SIZE`): indices cycle through at least this many slots
/// before reuse.
const MIN_CYCLE: usize = 64;
/// `ipcid_seq_max()`: `INT_MAX >> IPCMNI_SHIFT`.
const SEQ_MAX: u32 = (i32::MAX as u32) >> 15;

/// `S_IRUGO` and `S_IWUGO`, the access an operation asks `ipcperms` for.
pub(crate) const READ: u32 = 0o444;
pub(crate) const WRITE: u32 = 0o222;
/// `S_IRWXUGO`: the permission bits of `ipc_perm.mode`.
const MODE_BITS: u32 = 0o777;

/// The `*_STAT_ANY` commands, which the ABI does not name yet: `*_STAT` without the read
/// permission check.
pub(crate) const MSG_STAT_ANY: IpcCmd = IpcCmd(13);
pub(crate) const SHM_STAT_ANY: IpcCmd = IpcCmd(15);
pub(crate) const SEM_STAT_ANY: IpcCmd = IpcCmd(20);

/// Who acts on an IPC object: the caller's credentials, by kernel ids, and the user namespace
/// that owns its IPC namespace, over which `CAP_IPC_OWNER`, `CAP_SYS_ADMIN` and `CAP_IPC_LOCK`
/// count (`ns->user_ns`).
pub(crate) struct Cred {
    /// The filesystem ids, which own what the caller makes and are checked for access.
    pub(crate) fsuid: u32,
    pub(crate) fsgid: u32,
    /// The real uid, which message queues are charged to.
    pub(crate) uid: u32,
    cred: TaskCred,
    owner: Arc<UserNs>,
}

impl Cred {
    /// `in_group_p`.
    pub(crate) fn in_group(&self, gid: u32) -> bool {
        self.cred.in_group(Kgid(gid))
    }

    /// `ns_capable(ns->user_ns, cap)`: the capability over the IPC namespace's owner.
    pub(crate) fn ns_capable(&self, cap: Cap) -> bool {
        self.cred.ns_capable(&self.owner, cap)
    }

    /// `capable(cap)`: the capability over the initial user namespace, for the machine's
    /// limits.
    pub(crate) fn capable_init(&self, cap: Cap) -> bool {
        self.cred.capable_init(cap)
    }

    /// `capable_wrt_inode_uidgid`: the capability over the caller's user namespace, which
    /// maps `uid` and `gid`, for an object that is an inode.
    pub(crate) fn capable_wrt(&self, uid: u32, gid: u32, cap: Cap) -> bool {
        let ns = &self.cred.user_ns;
        self.cred.capable(cap) && ns.has_uid(Kuid(uid)) && ns.has_gid(Kgid(gid))
    }
}

impl Kernel {
    /// The caller as IPC checks it.
    pub(crate) fn ipc_cred(&self, caller: &Caller<'_>) -> Cred {
        let cred = self.cred(caller);
        let (fsuid, fsgid) = cred.fs_owner();
        Cred {
            fsuid,
            fsgid,
            uid: cred.user.real.0,
            owner: self.ns(caller).ipc.user_ns().clone(),
            cred,
        }
    }
}

/// What an IPC namespace holds ([`crate::ns::IpcNs`]): its SysV objects and POSIX message
/// queues.
#[derive(Default)]
pub(crate) struct Ipc {
    pub(crate) sem: sem::Sems,
    pub(crate) msg: msg::Queues,
    pub(crate) shm: Arc<shm::Segments>,
    pub(crate) mq: Arc<mqueue::Mqueues>,
}

/// `kern_ipc_perm`: who owns an object and who may use it, by kernel ids.
#[derive(Clone, Debug)]
pub(crate) struct Perm {
    pub(crate) key: i32,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) cuid: u32,
    pub(crate) cgid: u32,
    /// The permission bits and, for segments, `SHM_DEST` and `SHM_LOCKED`.
    pub(crate) mode: u32,
    pub(crate) seq: u16,
}

impl Perm {
    /// A new object's permissions: the creator owns it, with the low nine bits of `flags`.
    pub(crate) fn new(key: i32, cred: &Cred, flags: i32) -> Self {
        Self {
            key,
            uid: cred.fsuid,
            gid: cred.fsgid,
            cuid: cred.fsuid,
            cgid: cred.fsgid,
            mode: flags as u32 & MODE_BITS,
            seq: 0,
        }
    }

    /// `ipcperms`: whether `cred` has every access bit `flag` asks for.
    pub(crate) fn permits(&self, cred: &Cred, flag: u32) -> bool {
        let requested = (flag >> 6) | (flag >> 3) | flag;
        let granted = if cred.fsuid == self.cuid || cred.fsuid == self.uid {
            self.mode >> 6
        } else if cred.in_group(self.cgid) || cred.in_group(self.gid) {
            self.mode >> 3
        } else {
            self.mode
        };
        requested & !granted & 0o7 == 0 || cred.ns_capable(Cap::IpcOwner)
    }

    /// `ipcperms` as an errno.
    pub(crate) fn check(&self, cred: &Cred, flag: u32) -> Result<(), Errno> {
        match self.permits(cred, flag) {
            true => Ok(()),
            false => Err(Errno::EACCES),
        }
    }

    /// `ipcctl_obtain_check`: `IPC_SET` and `IPC_RMID` need the creator, the owner or
    /// `CAP_SYS_ADMIN` over the IPC namespace's owner.
    pub(crate) fn check_owner(&self, cred: &Cred) -> Result<(), Errno> {
        let admin = || cred.ns_capable(Cap::SysAdmin);
        match cred.fsuid == self.cuid || cred.fsuid == self.uid || admin() {
            true => Ok(()),
            false => Err(Errno::EPERM),
        }
    }

    /// `ipc_update_perm`: `IPC_SET` changes the owner, which the caller names in its user
    /// namespace `ns` and must be mapped there (`EINVAL`), and the permission bits.
    pub(crate) fn update(&mut self, from: &Ipc64Perm, ns: &UserNs) -> Result<(), Errno> {
        let uid = ns.make_kuid(from.uid).ok_or(Errno::EINVAL)?;
        let gid = ns.make_kgid(from.gid).ok_or(Errno::EINVAL)?;
        self.uid = uid.0;
        self.gid = gid.0;
        self.mode = (self.mode & !MODE_BITS) | (from.mode & MODE_BITS);
        Ok(())
    }

    /// `kernel_to_ipc64_perm`: the ids as the caller's user namespace `ns` sees them.
    pub(crate) fn to_abi(&self, ns: &UserNs) -> Ipc64Perm {
        Ipc64Perm {
            key: self.key,
            uid: ns.munged_uid(Kuid(self.uid)),
            gid: ns.munged_gid(Kgid(self.gid)),
            cuid: ns.munged_uid(Kuid(self.cuid)),
            cgid: ns.munged_gid(Kgid(self.cgid)),
            mode: self.mode,
            seq: self.seq,
            ..Ipc64Perm::default()
        }
    }
}

/// An object kept in [`Ids`]. Locks nest in one order: the table, then the object's state,
/// then its permissions.
pub(crate) trait Object {
    fn perm(&self) -> MutexGuard<'_, Perm>;
}

/// `struct ipc_ids`: the objects of one kind, by index and by key.
pub(crate) struct Ids<T> {
    slots: Vec<Option<Arc<T>>>,
    keys: HashMap<i32, usize>,
    in_use: usize,
    seq: u32,
    last_index: Option<usize>,
}

impl<T> Default for Ids<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            keys: HashMap::new(),
            in_use: 0,
            seq: 0,
            last_index: None,
        }
    }
}

/// The id of the object at `index` in generation `seq` (`ipc_buildid`).
pub(crate) fn build_id(index: usize, seq: u16) -> i32 {
    ((seq as usize) * IPCMNI + index) as i32
}

impl<T: Object> Ids<T> {
    /// The object named by `id`, if its index holds it in that id's generation
    /// (`ipc_obtain_object_check`).
    pub(crate) fn get(&self, id: i32) -> Option<Arc<T>> {
        let id = usize::try_from(id).ok()?;
        let object = self.slots.get(id % IPCMNI)?.as_ref()?;
        match object.perm().seq as usize == id / IPCMNI {
            true => Some(object.clone()),
            false => None,
        }
    }

    /// The object at `index`, for the `*_STAT` commands.
    pub(crate) fn at(&self, index: i32) -> Option<Arc<T>> {
        let index = usize::try_from(index).ok()?;
        self.slots.get(index)?.clone()
    }

    pub(crate) fn find_key(&self, key: i32) -> Option<Arc<T>> {
        self.slots[*self.keys.get(&key)?].clone()
    }

    /// `ipc_idr_alloc`: takes the next free index after the last one handed out, cycling
    /// through at least `MIN_CYCLE` slots, and bumps the generation when it wraps. `make` gets
    /// the generation and builds the object. `ENOSPC` once `limit` objects exist.
    pub(crate) fn add(
        &mut self,
        key: i32,
        limit: usize,
        make: impl FnOnce(u16) -> Arc<T>,
    ) -> Result<(i32, Arc<T>), Errno> {
        if self.in_use >= limit.min(IPCMNI) {
            return Err(Errno::ENOSPC);
        }
        let max = (self.in_use * 3 / 2).clamp(MIN_CYCLE, IPCMNI);
        let start = self.last_index.map_or(0, |last| last + 1);
        let free = |index: &usize| self.slots.get(*index).is_none_or(Option::is_none);
        let index = (start..max)
            .chain(0..start.min(max))
            .find(free)
            .ok_or(Errno::ENOSPC)?;
        if self.last_index.is_some_and(|last| index <= last) {
            self.seq = (self.seq + 1) % SEQ_MAX;
        }
        self.last_index = Some(index);
        let object = make(self.seq as u16);
        if self.slots.len() <= index {
            self.slots.resize_with(index + 1, || None);
        }
        self.slots[index] = Some(object.clone());
        if key != IpcKey::IPC_PRIVATE.0 {
            self.keys.insert(key, index);
        }
        self.in_use += 1;
        Ok((build_id(index, self.seq as u16), object))
    }

    /// `ipc_rmid`: the id and the key stop naming the object.
    pub(crate) fn remove(&mut self, id: i32) {
        let index = id as usize % IPCMNI;
        let Some(object) = self.slots.get_mut(index).and_then(Option::take) else {
            return;
        };
        let key = object.perm().key;
        if self.keys.get(&key) == Some(&index) {
            self.keys.remove(&key);
        }
        self.in_use -= 1;
    }

    /// `ipc_set_key_private`: the object stays, but its key no longer finds it.
    pub(crate) fn forget_key(&mut self, id: i32, object: &T) {
        let index = id as usize % IPCMNI;
        let mut perm = object.perm();
        if self.keys.get(&perm.key) == Some(&index) {
            self.keys.remove(&perm.key);
        }
        perm.key = IpcKey::IPC_PRIVATE.0;
    }

    pub(crate) fn in_use(&self) -> usize {
        self.in_use
    }

    /// `ipc_get_maxidx`: the highest index in use, which `IPC_INFO` returns; 0 when empty.
    pub(crate) fn max_index(&self) -> i32 {
        self.slots.iter().rposition(Option::is_some).unwrap_or(0) as i32
    }

    /// Every object with its id, in index order, for `/proc/sysvipc` and the `*_INFO` totals.
    pub(crate) fn all(&self) -> Vec<(i32, Arc<T>)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                let object = slot.as_ref()?;
                Some((build_id(index, object.perm().seq), object.clone()))
            })
            .collect()
    }
}

/// `ipcget`: finds the object `key` names or creates one. `check` runs on an existing object
/// before the permission check (`more_checks`, so a size that does not fit is `EINVAL` before
/// `EACCES`); `create` builds a new one from its permissions.
pub(crate) fn get_or_create<T: Object>(
    ids: &Mutex<Ids<T>>,
    key: i32,
    flags: i32,
    cred: &Cred,
    limit: usize,
    check: impl FnOnce(&T) -> Result<(), Errno>,
    create: impl FnOnce(Perm) -> Result<T, Errno>,
) -> Result<i32, Errno> {
    let flags_bits = IpcFlags(flags as u32);
    let mut ids = ids.lock_or_recover();
    if key != IpcKey::IPC_PRIVATE.0
        && let Some(object) = ids.find_key(key)
    {
        if flags_bits.contains(IpcFlags::IPC_CREAT | IpcFlags::IPC_EXCL) {
            return Err(Errno::EEXIST);
        }
        check(&object)?;
        let perm = object.perm();
        perm.check(cred, flags as u32)?;
        return Ok(build_id(
            *ids.keys.get(&key).expect("found by key"),
            perm.seq,
        ));
    }
    if key != IpcKey::IPC_PRIVATE.0 && !flags_bits.contains(IpcFlags::IPC_CREAT) {
        return Err(Errno::ENOENT);
    }
    let object = create(Perm::new(key, cred, flags))?;
    let (id, _) = ids.add(key, limit, |seq| {
        object.perm().seq = seq;
        Arc::new(object)
    })?;
    Ok(id)
}

impl crate::Kernel {
    /// Seconds of `CLOCK_REALTIME`, what the IPC timestamps record.
    pub(crate) fn ipc_now(&self) -> u64 {
        self.host.realtime().as_secs()
    }
}

/// Seconds of `CLOCK_REALTIME`, split as the 32-bit `*_ds` structures store them.
pub(crate) fn split_time(secs: u64) -> (u32, u32) {
    (secs as u32, (secs >> 32) as u32)
}
