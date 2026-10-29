//! Linux FUSE (fs/fuse/{dev,inode,dir,file,xattr,readdir,control}.c).
//! The device protocol is private byte I/O, independent of the typed guest ABI.
mod control;
mod dev;
pub(crate) use control::{ControlType, Registry};
mod fs;
mod poll;
mod wire;

use crate::{
    Kernel, TaskTable,
    fs::mount::{FilesystemType, FsParam},
    ns::{PidNs, UserNs},
    signal::TaskSignals,
    sync::MutexExt,
};
pub(crate) use dev::Device;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, AtomicUsize},
    },
    time::Duration,
};
use wasixv2_host::Host;
use wasixv2_syscalls::{Errno, TaskId, current_task};
use wasixv2_vfs::{Cred, FileType, Filesystem};
use wasixv2_wait::WaitQueue;
use wire::*;

// BIG_WRITES and DO_READDIRPLUS. In particular no WRITEBACK_CACHE, POSIX_ACL,
// passthrough, splice, DAX, or mmap of direct I/O files (fuse_send_init).
const INIT_FLAGS: u32 = (1 << 5) | (1 << 13);

pub(super) struct Connection {
    sync_init: bool,
    queue: Mutex<dev::Queue>,
    readable: WaitQueue,
    changed: WaitQueue,
    devices: AtomicUsize,
    state: Weak<TaskTable>,
    host: Arc<dyn Host>,
    user_ns: Arc<UserNs>,
    pid_ns: Arc<PidNs>,
    options: Options,
    nodes: Mutex<BTreeMap<u64, Weak<fs::FuseNode>>>,
    unsupported: Mutex<BTreeSet<u32>>,
    polls: Mutex<BTreeMap<u64, Weak<poll::PollState>>>,
    next_poll: AtomicU64,
}
impl Connection {
    fn current(&self) -> (u32, u32, u32, Option<Arc<TaskSignals>>) {
        let Some(state) = self.state.upgrade() else {
            return (0, 0, 0, None);
        };
        let state = state.read_or_recover();
        let Some(task) = current_task().and_then(|id| state.tasks.get(&id)) else {
            return (0, 0, 0, None);
        };
        let cred = task.process.cred();
        (
            self.user_ns.munged_uid(cred.user.fs),
            self.user_ns.munged_gid(cred.group.fs),
            state.pids.nr(&self.pid_ns, task.process.pid.0) as u32,
            Some(task.signals.clone()),
        )
    }

    /// fs/fuse/dir.c:fuse_allow_current_process, fuse_permissible_uidgid.
    fn allowed(&self) -> Result<(), Errno> {
        let Some(state) = self.state.upgrade() else {
            return Err(Errno::ENOTCONN);
        };
        let state = state.read_or_recover();
        let Some(task) = current_task().and_then(|id| state.tasks.get(&id)) else {
            return Ok(());
        };
        let cred = task.process.cred();
        if self.options.allow_other {
            let mut ns = Some(&cred.user_ns);
            while let Some(at) = ns {
                if Arc::ptr_eq(at, &self.user_ns) {
                    return Ok(());
                }
                ns = at.parent();
            }
        } else if [cred.user.real, cred.user.effective, cred.user.saved]
            .iter()
            .all(|id| id.0 == self.options.uid)
            && [cred.group.real, cred.group.effective, cred.group.saved]
                .iter()
                .all(|id| id.0 == self.options.gid)
        {
            return Ok(());
        }
        Err(Errno::EACCES)
    }
    fn deadline(&self, secs: u64, nanos: u32) -> Duration {
        self.host
            .monotonic()
            .saturating_add(Duration::new(secs, nanos.min(999_999_999)))
    }
    fn invalidate(&self, nodeid: u64) {
        if let Some(node) = self
            .nodes
            .lock_or_recover()
            .get(&nodeid)
            .and_then(Weak::upgrade)
        {
            node.invalidate();
        }
    }
    fn notify(&self, code: i32, body: &[u8]) -> Result<(), Errno> {
        // fs/fuse/dev.c:fuse_notify: positive error with unique zero is a notification.
        match code {
            1 if body.len() == 8 => {
                if let Some(poll) = self
                    .polls
                    .lock_or_recover()
                    .get(&u64_at(body, 0)?)
                    .and_then(Weak::upgrade)
                {
                    poll.notify();
                }
                Ok(())
            }
            2 if body.len() == 24 => {
                self.invalidate(u64_at(body, 0)?);
                Ok(())
            }
            3 | 6 => {
                let start = if code == 6 { 24 } else { 16 };
                let len = u32_at(body, if code == 6 { 16 } else { 8 })? as usize;
                if body.len() != start + len + 1 || body.last() != Some(&0) {
                    return Err(Errno::EINVAL);
                }
                if let Some(node) = self
                    .nodes
                    .lock_or_recover()
                    .get(&u64_at(body, 0)?)
                    .and_then(Weak::upgrade)
                {
                    node.invalidate_entry(&body[start..start + len]);
                }
                if code == 6 {
                    self.invalidate(u64_at(body, 8)?);
                }
                Ok(())
            }
            _ => Err(Errno::EINVAL),
        }
    }
    async fn optional(
        &self,
        opcode: u32,
        node: u64,
        body: Vec<u8>,
        min: usize,
        max: usize,
    ) -> Result<Vec<u8>, Errno> {
        if self.unsupported.lock_or_recover().contains(&opcode) {
            return Err(Errno::ENOSYS);
        }
        let result = self.request(opcode, node, body, min, max).await;
        if result == Err(Errno::ENOSYS) {
            self.unsupported.lock_or_recover().insert(opcode);
        }
        result
    }
}

#[derive(Default, Clone, Debug)]
struct Options {
    fd: Option<u32>,
    mode: Option<u32>,
    uid: u32,
    gid: u32,
    uid_present: bool,
    gid_present: bool,
    allow_other: bool,
    default_permissions: bool,
    max_read: Option<u32>,
    subtype: Option<Vec<u8>>,
}
impl Options {
    /// fs/fuse/inode.c:fuse_parse_param (rootmode is octal, other integers decimal).
    fn parse(&mut self, param: &FsParam<'_>) -> Result<(), Errno> {
        let number = |radix| {
            let value = param.value.ok_or(Errno::EINVAL)?;
            if value.is_empty() || value.first() == Some(&b'-') {
                return Err(Errno::EINVAL);
            }
            u32::from_str_radix(
                std::str::from_utf8(value).map_err(|_| Errno::EINVAL)?,
                radix,
            )
            .map_err(|_| Errno::EINVAL)
        };
        match param.key {
            b"fd" => {
                if self.fd.replace(number(10)?).is_some() {
                    return Err(Errno::EINVAL);
                }
            }
            b"rootmode" => {
                let mode = number(8)?;
                FileType::from_mode(mode).ok_or(Errno::EINVAL)?;
                self.mode = Some(mode);
            }
            b"user_id" => {
                self.uid = number(10)?;
                self.uid_present = true;
            }
            b"group_id" => {
                self.gid = number(10)?;
                self.gid_present = true;
            }
            b"max_read" => self.max_read = Some(number(10)?.max(4096)),
            b"allow_other" if param.value.is_none() => self.allow_other = true,
            b"default_permissions" if param.value.is_none() => self.default_permissions = true,
            // fuseblk alone accepts blksize; fuse_parse_param rejects it for fuse.
            b"subtype" if self.subtype.is_none() => {
                self.subtype = Some(param.value.ok_or(Errno::EINVAL)?.to_vec())
            }
            _ => return Err(Errno::EINVAL),
        }
        Ok(())
    }
}

pub(crate) struct FuseType {
    pub(crate) state: Weak<TaskTable>,
    pub(crate) host: Arc<dyn Host>,
    pub(crate) registry: Arc<Registry>,
}
impl FilesystemType for FuseType {
    fn userns_mount(&self) -> bool {
        true
    }
    fn parse_param(&self, param: &FsParam<'_>) -> Result<(), Errno> {
        Options::default().parse(param)
    }
    fn mount(
        &self,
        _: &[u8],
        params: &[FsParam<'_>],
        _: &Cred,
    ) -> Result<Arc<dyn Filesystem>, Errno> {
        let mut options = Options::default();
        for param in params {
            options.parse(param)?;
        }
        if options.mode.is_none() || !options.uid_present || !options.gid_present {
            return Err(Errno::EINVAL);
        }
        let table = self.state.upgrade().ok_or(Errno::ENODEV)?;
        let (file, user_ns, pid_ns) = {
            let state = table.read_or_recover();
            let task = state
                .tasks
                .get(&current_task().ok_or(Errno::ESRCH)?)
                .ok_or(Errno::ESRCH)?;
            let file = task
                .files
                .lock_or_recover()
                .get(wasixv2_syscalls::Fd(options.fd.ok_or(Errno::EINVAL)? as i32))
                .ok_or(Errno::EBADF)?;
            let user_ns = task.process.cred().user_ns;
            (file, user_ns, task.process.pid_ns.clone())
        };
        let device = file.downcast::<Device>().ok_or(Errno::EINVAL)?;
        if !Arc::ptr_eq(&user_ns, &device.user_ns) {
            return Err(Errno::EINVAL);
        }
        options.uid = user_ns.make_kuid(options.uid).ok_or(Errno::EINVAL)?.0;
        options.gid = user_ns.make_kgid(options.gid).ok_or(Errno::EINVAL)?.0;
        let mut bound = device.conn.lock_or_recover();
        if bound.is_some() {
            return Err(Errno::EBUSY);
        }
        let conn = Arc::new(Connection {
            sync_init: device.sync_init.load(std::sync::atomic::Ordering::Acquire),
            queue: Mutex::new(dev::Queue::default()),
            readable: WaitQueue::new(),
            changed: WaitQueue::new(),
            devices: AtomicUsize::new(1),
            state: self.state.clone(),
            host: self.host.clone(),
            user_ns,
            pid_ns,
            options,
            nodes: Mutex::new(BTreeMap::new()),
            unsupported: Mutex::new(BTreeSet::new()),
            polls: Mutex::new(BTreeMap::new()),
            next_poll: AtomicU64::new(1),
        });
        self.registry.add(&conn);
        let fs = fs::FuseFs::new(conn.clone());
        let mut init = vec![0; 64];
        put32(&mut init, 0, 7);
        put32(&mut init, 4, MINOR);
        put32(&mut init, 12, INIT_FLAGS | (1 << 30));
        conn.enqueue(INIT, 0, init, 24, 64);
        *bound = Some(conn);
        Ok(Arc::new(fs))
    }
}
impl Kernel {
    pub(crate) async fn clone_fuse(
        &self,
        caller: &wasixv2_syscalls::Caller<'_>,
        device: &Device,
        arg: u32,
    ) -> wasixv2_syscalls::SysResult<i32> {
        let fd: u32 =
            wasixv2_syscalls::read_value(caller.memory(), wasixv2_abi::GuestPtr::new(arg)).await?;
        let file = self
            .file(caller.task(), wasixv2_syscalls::Fd(fd as i32))
            .map_err(|_| Errno::EINVAL)?;
        let source = file.downcast::<Device>().ok_or(Errno::EINVAL)?;
        let conn = source.conn.lock_or_recover().clone().ok_or(Errno::EPERM)?;
        let mut bound = device.conn.lock_or_recover();
        if bound.is_some() {
            return Err(Errno::EINVAL.into());
        }
        conn.devices
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        *bound = Some(conn);
        Ok(0)
    }
    pub(crate) fn open_fuse(&self, task: TaskId) -> Arc<Device> {
        Arc::new(Device {
            conn: Mutex::new(None),
            user_ns: self.process(task).cred().user_ns,
            sync_init: std::sync::atomic::AtomicBool::new(false),
        })
    }
}
