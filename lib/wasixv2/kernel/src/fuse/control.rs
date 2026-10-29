//! fs/fuse/control.c: connection discovery, waiting requests and abort.
use super::Connection;
use crate::{
    fs::mount::{FilesystemType, FsParam},
    sync::MutexExt,
};
use async_trait::async_trait;
use std::{
    any::Any,
    collections::BTreeMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
};
use wasixv2_syscalls::Errno;
use wasixv2_vfs::{
    Attr, Cred, Device, DirEntry, DirSink, FileType, Filesystem, FsInfo, FsResult, Handle, Node,
    OpenOptions, SetAttr, StatFs, Timestamp,
};

#[derive(Default)]
pub(crate) struct Registry {
    next: AtomicU64,
    connections: Mutex<BTreeMap<u64, Weak<Connection>>>,
}
impl Registry {
    pub(super) fn add(&self, conn: &Arc<Connection>) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.connections
            .lock_or_recover()
            .insert(id, Arc::downgrade(conn));
        id
    }
    fn list(&self) -> Vec<u64> {
        let mut connections = self.connections.lock_or_recover();
        connections.retain(|_, conn| conn.strong_count() > 0);
        connections.keys().copied().collect()
    }
    fn get(&self, id: u64) -> FsResult<Arc<Connection>> {
        self.connections
            .lock_or_recover()
            .get(&id)
            .and_then(Weak::upgrade)
            .ok_or(Errno::ENOENT)
    }
}

pub(crate) struct ControlType(pub(crate) Arc<Registry>);
impl FilesystemType for ControlType {
    fn mount(&self, _: &[u8], params: &[FsParam<'_>], _: &Cred) -> FsResult<Arc<dyn Filesystem>> {
        if !params.is_empty() {
            return Err(Errno::EINVAL);
        }
        Ok(Arc::new(ControlFs(self.0.clone())))
    }
}
struct ControlFs(Arc<Registry>);
#[async_trait(?Send)]
impl Filesystem for ControlFs {
    fn info(&self) -> FsInfo {
        FsInfo {
            name: "fusectl",
            magic: 0x65735543,
            name_max: 255,
            read_only: false,
            dev: None,
            has_btime: false,
            stable: false,
        }
    }
    fn root(&self) -> Arc<dyn Node> {
        Arc::new(ControlNode {
            registry: self.0.clone(),
            id: 0,
            file: None,
        })
    }
    async fn statfs(&self) -> FsResult<StatFs> {
        Ok(StatFs {
            block_size: 4096,
            fragment_size: 4096,
            name_max: 255,
            ..StatFs::default()
        })
    }
}
const FILES: &[(&[u8], u32)] = &[
    (b"waiting", 0o400),
    (b"abort", 0o200),
    (b"max_background", 0o600),
    (b"congestion_threshold", 0o600),
];
#[derive(Clone)]
struct ControlNode {
    registry: Arc<Registry>,
    id: u64,
    file: Option<usize>,
}
impl ControlNode {
    fn conn(&self) -> FsResult<Arc<Connection>> {
        self.registry.get(self.id)
    }
}
#[async_trait(?Send)]
impl Node for ControlNode {
    fn ino(&self) -> u64 {
        if self.id == 0 {
            1
        } else {
            self.id * 8 + 2 + self.file.map_or(0, |i| i as u64 + 1)
        }
    }
    fn file_type(&self) -> FileType {
        if self.file.is_some() {
            FileType::Regular
        } else {
            FileType::Directory
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    async fn getattr(&self) -> FsResult<Attr> {
        let (uid, gid) = if self.id == 0 {
            (0, 0)
        } else {
            let c = self.conn()?;
            (c.options.uid, c.options.gid)
        };
        Ok(Attr {
            ino: self.ino(),
            file_type: self.file_type(),
            mode: self.file.map_or(0o500, |f| FILES[f].1),
            uid,
            gid,
            nlink: if self.file.is_some() { 1 } else { 2 },
            rdev: Device::default(),
            size: 0,
            blocks: 0,
            blksize: 4096,
            atime: Timestamp::default(),
            mtime: Timestamp::default(),
            ctime: Timestamp::default(),
            btime: None,
            dev: None,
        })
    }
    async fn setattr(&self, _: &SetAttr) -> FsResult<Attr> {
        Err(Errno::EPERM)
    }
    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        if self.file.is_some() {
            return Err(Errno::ENOTDIR);
        }
        let child = if self.id == 0 {
            let id = std::str::from_utf8(name)
                .ok()
                .and_then(|n| n.parse::<u64>().ok())
                .filter(|id| id.to_string().as_bytes() == name)
                .ok_or(Errno::ENOENT)?;
            self.registry.get(id)?;
            Self {
                registry: self.registry.clone(),
                id,
                file: None,
            }
        } else {
            self.conn()?;
            let file = FILES
                .iter()
                .position(|(n, _)| *n == name)
                .ok_or(Errno::ENOENT)?;
            Self {
                file: Some(file),
                ..self.clone()
            }
        };
        Ok(Arc::new(child))
    }
    async fn open(&self, _: OpenOptions) -> FsResult<Box<dyn Handle>> {
        if self.id != 0 {
            self.conn()?;
        }
        Ok(Box::new(self.clone()))
    }
}
#[async_trait(?Send)]
impl Handle for ControlNode {
    async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let conn = self.conn()?;
        let q = conn.queue.lock_or_recover();
        let value = match self.file {
            Some(0) => q.waiting(),
            Some(2) => q.max_background as usize,
            Some(3) => q.congestion_threshold as usize,
            _ => return Err(Errno::EINVAL),
        };
        let text = format!("{value}\n");
        let start = (offset as usize).min(text.len());
        let n = buf.len().min(text.len() - start);
        buf[..n].copy_from_slice(&text.as_bytes()[start..start + n]);
        Ok(n)
    }
    async fn write_at(&self, data: &[u8], _: u64) -> FsResult<usize> {
        let conn = self.conn()?;
        if self.file == Some(1) {
            conn.abort();
            return Ok(data.len());
        }
        let value = std::str::from_utf8(data)
            .ok()
            .and_then(|s| s.trim().parse::<u16>().ok())
            .filter(|v| *v != 0)
            .ok_or(Errno::EINVAL)?;
        let mut q = conn.queue.lock_or_recover();
        match self.file {
            Some(2) => q.max_background = value,
            Some(3) => q.congestion_threshold = value,
            _ => return Err(Errno::EINVAL),
        };
        Ok(data.len())
    }
    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()> {
        if self.file.is_some() {
            return Err(Errno::ENOTDIR);
        }
        let names: Vec<Vec<u8>> = if self.id == 0 {
            self.registry
                .list()
                .into_iter()
                .map(|n| n.to_string().into_bytes())
                .collect()
        } else {
            self.conn()?;
            FILES.iter().map(|(n, _)| n.to_vec()).collect()
        };
        for (i, name) in names.iter().enumerate().skip(cookie as usize) {
            let node = Node::lookup(self, name).await?;
            if !sink.push(DirEntry {
                name,
                ino: node.ino(),
                file_type: node.file_type(),
                next: i as u64 + 1,
            }) {
                break;
            }
        }
        Ok(())
    }
}
