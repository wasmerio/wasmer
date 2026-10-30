//! Native-testable provider using existing drivers, also usable in a browser runtime heap.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use wasixv2_abi::Errno;
use wasixv2_vfs::{DirEntry, DirSink, FileType, Filesystem, FsResult, Handle, Node, RenameMode};

use crate::{MAX_IO, READDIR_BATCH, WebFs, protocol::*};

/// Capabilities of the existing memory driver (including its explicit lack of whiteouts).
pub const MEMORY_CAPABILITIES: Capabilities = Capabilities {
    hard_links: true,
    symlinks: true,
    special_files: true,
    xattrs: true,
    atomic_append: true,
    rename_noreplace: true,
    rename_exchange: true,
    unnamed_files: true,
    allocation: true,
};

/// An in-process provider. All namespace/ACL edits of the backing driver must pass through
/// this provider, including embedder edits. This lets several VFS clients share memfs/overlay
/// safely without independently trusting checks made under another VFS's inode locks.
pub struct LocalProvider {
    fs: Arc<dyn Filesystem>,
    objects: Mutex<Objects>,
    /// Async, not a synchronous mutex held across a driver future. Like lock_rename plus
    /// i_rwsem, serializes checks and namespace changes across independent mounted clients.
    namespace: futures::lock::Mutex<()>,
    /// Directory ancestry, learned as directories are acquired, for lock_rename's trap check.
    parents: Mutex<HashMap<u64, u64>>,
    version: AtomicU64,
}

#[derive(Default)]
struct Objects {
    next: u64,
    nodes: HashMap<ObjectId, Arc<dyn Node>>,
    handles: HashMap<ObjectId, Arc<dyn Handle>>,
}

impl Objects {
    fn id(&mut self) -> FsResult<ObjectId> {
        self.next = self.next.checked_add(1).ok_or(Errno::ENFILE)?;
        Ok(ObjectId(self.next))
    }
}

impl LocalProvider {
    #[cfg(test)]
    pub(crate) fn retained(&self) -> (usize, usize) {
        let objects = self.objects.lock().unwrap();
        (objects.nodes.len(), objects.handles.len())
    }

    pub fn new(fs: Arc<dyn Filesystem>) -> Arc<Self> {
        let root = fs.root().ino();
        Arc::new(Self {
            fs,
            objects: Mutex::new(Objects::default()),
            namespace: futures::lock::Mutex::new(()),
            parents: Mutex::new(HashMap::from([(root, root)])),
            version: AtomicU64::new(0),
        })
    }

    /// Wraps an existing driver, retaining its metadata and explicit capabilities. Edits
    /// through this mount use the same protocol as an out-of-process JS provider.
    pub async fn mount(fs: Arc<dyn Filesystem>, caps: Capabilities) -> FsResult<Arc<WebFs>> {
        let info = fs.info();
        WebFs::new(Self::new(fs), info, caps).await
    }

    fn node(&self, id: ObjectId) -> FsResult<Arc<dyn Node>> {
        self.objects
            .lock()
            .unwrap()
            .nodes
            .get(&id)
            .cloned()
            .ok_or(Errno::ESTALE)
    }

    fn handle(&self, id: ObjectId) -> FsResult<Arc<dyn Handle>> {
        self.objects
            .lock()
            .unwrap()
            .handles
            .get(&id)
            .cloned()
            .ok_or(Errno::EBADF)
    }

    fn retain_node(&self, node: Arc<dyn Node>) -> FsResult<NodeRef> {
        let mut objects = self.objects.lock().unwrap();
        let id = objects.id()?;
        let reference = NodeRef {
            id,
            ino: node.ino(),
            generation: node.generation(),
            file_type: node.file_type(),
        };
        objects.nodes.insert(id, node);
        Ok(reference)
    }

    fn retain_handle(&self, handle: Box<dyn Handle>) -> FsResult<ObjectId> {
        let mut objects = self.objects.lock().unwrap();
        let id = objects.id()?;
        objects.handles.insert(id, Arc::from(handle));
        Ok(id)
    }

    fn created(&self, node: Arc<dyn Node>, handle: Box<dyn Handle>) -> FsResult<Reply> {
        let node = self.retain_node(node)?;
        match self.retain_handle(handle) {
            Ok(handle) => Ok(Reply::Created(node, handle)),
            Err(error) => {
                self.release(Resource::Node(node.id));
                Err(error)
            }
        }
    }

    async fn directory(&self, id: ObjectId) -> FsResult<Arc<dyn Node>> {
        let node = self.node(id)?;
        if node.file_type() != FileType::Directory {
            return Err(Errno::ENOTDIR);
        }
        // S_DEAD on each VFS's inode does not cover removal by a different VFS client.
        // Linux fs/namei.c:may_create_dentry rejects creation in an unlinked directory.
        if node.getattr().await?.nlink == 0 {
            return Err(Errno::ENOENT);
        }
        Ok(node)
    }

    fn parent(&self, node: &Arc<dyn Node>, parent: &Arc<dyn Node>) {
        if node.file_type() == FileType::Directory {
            self.parents
                .lock()
                .unwrap()
                .insert(node.ino(), parent.ino());
        }
    }

    fn below(&self, mut child: u64, ancestor: u64) -> bool {
        let parents = self.parents.lock().unwrap();
        loop {
            if child == ancestor {
                return true;
            }
            match parents.get(&child) {
                Some(&parent) if parent != child => child = parent,
                _ => return false,
            }
        }
    }

    async fn rename(
        &self,
        dir: ObjectId,
        name: &[u8],
        new_dir: ObjectId,
        new_name: &[u8],
        mode: RenameMode,
    ) -> FsResult<()> {
        let dir = self.directory(dir).await?;
        let new_dir = self.directory(new_dir).await?;
        let source = dir.lookup(name).await?;
        let target = match new_dir.lookup(new_name).await {
            Ok(target) => Some(target),
            Err(Errno::ENOENT) => None,
            Err(error) => return Err(error),
        };
        // Checks cannot rely on either caller's dentry tree: another client can rename it.
        // Linux fs/namei.c:lock_rename, vfs_rename (source/target trap and type checks).
        if source.file_type() == FileType::Directory && self.below(new_dir.ino(), source.ino()) {
            return Err(Errno::EINVAL);
        }
        if let Some(target) = &target {
            if mode == RenameMode::NoReplace {
                return Err(Errno::EEXIST);
            }
            if mode == RenameMode::Exchange {
                if target.file_type() == FileType::Directory && self.below(dir.ino(), target.ino())
                {
                    return Err(Errno::EINVAL);
                }
            } else if source.file_type() == FileType::Directory
                && target.file_type() != FileType::Directory
            {
                return Err(Errno::ENOTDIR);
            } else if source.file_type() != FileType::Directory
                && target.file_type() == FileType::Directory
            {
                return Err(Errno::EISDIR);
            }
        }
        dir.rename(name, &new_dir, new_name, mode).await?;
        if let Some(target) = &target {
            if source.ino() == target.ino() {
                return Ok(());
            }
            if mode == RenameMode::Exchange {
                self.parent(target, &dir);
            } else if target.file_type() == FileType::Directory {
                self.parents.lock().unwrap().remove(&target.ino());
            }
        }
        self.parent(&source, &new_dir);
        Ok(())
    }

    async fn dispatch(&self, request: Request) -> FsResult<Reply> {
        Ok(match request {
            Request::Root => Reply::Node(self.retain_node(self.fs.root())?),
            Request::StatFs => Reply::StatFs(self.fs.statfs().await?),
            Request::SyncFs => {
                self.fs.sync().await?;
                Reply::Done
            }
            Request::GetAttr(id) => Reply::Attr(self.node(id)?.getattr().await?),
            Request::SetAttr(id, attr) => {
                let attr = self.node(id)?.setattr(&attr).await?;
                self.version.fetch_add(1, Ordering::AcqRel);
                Reply::Attr(attr)
            }
            Request::Lookup(id, name) => {
                let _guard = self.namespace.lock().await;
                let dir = self.node(id)?;
                let node = dir.lookup(&name).await?;
                self.parent(&node, &dir);
                Reply::Node(self.retain_node(node)?)
            }
            Request::Create(id, name, new) => {
                let _guard = self.namespace.lock().await;
                let dir = self.directory(id).await?;
                let node = dir.create(&name, &new.as_new()).await?;
                self.parent(&node, &dir);
                Reply::Node(self.retain_node(node)?)
            }
            Request::CreateOpen(id, name, new, options) => {
                let _guard = self.namespace.lock().await;
                let dir = self.directory(id).await?;
                let (node, handle) = dir.create_open(&name, &new.as_new(), options).await?;
                self.parent(&node, &dir);
                self.created(node, handle)?
            }
            Request::CreateUnnamed(id, new, options) => {
                let _guard = self.namespace.lock().await;
                let (node, handle) = self
                    .directory(id)
                    .await?
                    .create_unnamed(&new.as_new(), options)
                    .await?;
                self.created(node, handle)?
            }
            Request::Link(id, name, target) => {
                let _guard = self.namespace.lock().await;
                let target = self.node(target)?;
                if target.file_type() == FileType::Directory {
                    return Err(Errno::EPERM);
                }
                self.directory(id).await?.link(&name, &target).await?;
                Reply::Done
            }
            Request::Unlink(id, name) => {
                let _guard = self.namespace.lock().await;
                self.directory(id).await?.unlink(&name).await?;
                Reply::Done
            }
            Request::Rmdir(id, name) => {
                let _guard = self.namespace.lock().await;
                let dir = self.directory(id).await?;
                let node = dir.lookup(&name).await?;
                dir.rmdir(&name).await?;
                self.parents.lock().unwrap().remove(&node.ino());
                Reply::Done
            }
            Request::Rename(id, name, new_dir, new_name, mode) => {
                let _guard = self.namespace.lock().await;
                self.rename(id, &name, new_dir, &new_name, mode).await?;
                Reply::Done
            }
            Request::ReadLink(id) => Reply::Bytes(self.node(id)?.readlink().await?),
            Request::Open(id, options) => {
                Reply::Open(self.retain_handle(self.node(id)?.open(options).await?)?)
            }
            Request::GetXattr(id, name) => Reply::Bytes(self.node(id)?.getxattr(&name).await?),
            Request::SetXattr(id, name, value, how) => {
                self.node(id)?.setxattr(&name, &value, how).await?;
                self.version.fetch_add(1, Ordering::AcqRel);
                Reply::Done
            }
            Request::ListXattr(id) => Reply::Names(self.node(id)?.listxattr().await?),
            Request::RemoveXattr(id, name) => {
                self.node(id)?.removexattr(&name).await?;
                self.version.fetch_add(1, Ordering::AcqRel);
                Reply::Done
            }
            Request::Read(id, offset, len) => {
                if len > MAX_IO {
                    return Err(Errno::EINVAL);
                }
                let mut bytes = vec![0; len];
                let n = self.handle(id)?.read_at(&mut bytes, offset).await?;
                if n > bytes.len() {
                    return Err(Errno::EIO);
                }
                bytes.truncate(n);
                Reply::Bytes(bytes)
            }
            Request::Write(id, offset, bytes) => {
                if bytes.len() > MAX_IO {
                    return Err(Errno::EINVAL);
                }
                Reply::Count(self.handle(id)?.write_at(&bytes, offset).await?)
            }
            Request::Append(id, bytes) => {
                if bytes.len() > MAX_IO {
                    return Err(Errno::EINVAL);
                }
                let (at, n) = self.handle(id)?.append(&bytes).await?;
                Reply::Appended(at, n)
            }
            Request::Sync(id, data_only) => {
                self.handle(id)?.sync(data_only).await?;
                Reply::Done
            }
            Request::SetSize(id, size) => {
                self.handle(id)?.set_size(size).await?;
                Reply::Done
            }
            Request::Allocate(id, mode, offset, len) => {
                self.handle(id)?.allocate(mode, offset, len).await?;
                Reply::Done
            }
            Request::SeekHole(id, offset, size, which) => {
                Reply::Offset(self.handle(id)?.seek_hole(offset, size, which).await?)
            }
            Request::ReadDir(id, cookie, max) => {
                if max == 0 || max > READDIR_BATCH {
                    return Err(Errno::EINVAL);
                }
                let mut entries = Entries {
                    entries: Vec::new(),
                    max,
                };
                self.handle(id)?.readdir(cookie, &mut entries).await?;
                Reply::Entries(entries.entries)
            }
        })
    }
}

#[async_trait(?Send)]
impl Provider for LocalProvider {
    async fn call(&self, request: Request) -> FsResult<Reply> {
        self.dispatch(request).await
    }

    fn release(&self, resource: Resource) {
        // Drop objects outside the registry lock; their destructors can release other resources.
        match resource {
            Resource::Node(id) => {
                let node = self.objects.lock().unwrap().nodes.remove(&id);
                drop(node);
            }
            Resource::Handle(id) => {
                let handle = self.objects.lock().unwrap().handles.remove(&id);
                drop(handle);
            }
        }
    }

    fn acl_version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }
}

struct Entries {
    entries: Vec<Entry>,
    max: usize,
}

impl DirSink for Entries {
    fn push(&mut self, entry: DirEntry<'_>) -> bool {
        if self.entries.len() == self.max {
            return false;
        }
        self.entries.push(Entry {
            name: entry.name.to_vec(),
            ino: entry.ino,
            file_type: entry.file_type,
            next: entry.next,
        });
        true
    }
}
