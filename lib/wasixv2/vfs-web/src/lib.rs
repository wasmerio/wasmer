//! Browser directory providers behind the existing VFS traits.
//!
//! [`memory`] supplies a shared writable mount using the existing memory driver. [`WebFs`]
//! accepts a JS provider through the owned [`protocol`] records; JS objects remain on that
//! provider's event loop. No browser, executor, engine or guest-memory contract is required
//! by this crate. Mount the same `Arc<WebFs>` wherever a shared directory is needed.
//!
//! VFS retains path walking, permissions and containment (`fs/namei.c:link_path_walk`,
//! `fs/open.c:do_sys_openat2`). Providers receive one byte component or a held object ID.
//! Names always revalidate, since the embedder may edit a mounted directory.

use std::{
    any::Any,
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};

use async_trait::async_trait;
use wasixv2_abi::Errno;
use wasixv2_vfs::{
    AllocMode, Attr, Clock, DirEntry, DirSink, FileType, Filesystem, FsInfo, FsResult, Handle,
    MAX_DIR_COOKIE, NAME_MAX, NewNode, Node, OpenOptions, RenameMode, SeekHole, SetAttr, StatFs,
    XattrSet,
};
use wasixv2_vfs_mem::MemFs;

mod local;
pub mod protocol;

pub use local::LocalProvider;
pub use local::MEMORY_CAPABILITIES;
use protocol::{Capabilities, Create, NodeRef, ObjectId, Provider, Reply, Request, Resource};

/// Maximum bytes in one provider data request. Larger I/O returns a short count.
pub const MAX_IO: usize = 65536;
/// Maximum directory records in one provider reply.
pub const READDIR_BATCH: usize = 64;

/// A mounted browser directory. Its root is acquired before it is exposed to VFS.
pub struct WebFs {
    inner: Arc<Inner>,
    root: Arc<dyn Node>,
}

struct Inner {
    provider: Arc<dyn Provider>,
    info: FsInfo,
    caps: Capabilities,
    nodes: Mutex<HashMap<u64, Weak<WebNode>>>,
}

/// A shared, in-memory directory. The returned mount can outlive every guest process.
/// The embedder edits it through VFS with its explicit credentials and root, just as a guest
/// does; no host path traversal API or second filesystem implementation is involved.
pub async fn memory(clock: Arc<dyn Clock>, mode: u32, uid: u32, gid: u32) -> FsResult<Arc<WebFs>> {
    LocalProvider::mount(
        Arc::new(MemFs::new(clock, mode, uid, gid)),
        MEMORY_CAPABILITIES,
    )
    .await
}

impl WebFs {
    /// Connects a provider with explicit filesystem metadata and capability limits.
    /// `stable` is always false: cached names cannot hide embedder-side edits.
    pub async fn new(
        provider: Arc<dyn Provider>,
        mut info: FsInfo,
        caps: Capabilities,
    ) -> FsResult<Arc<Self>> {
        if info.name_max == 0 || info.name_max > NAME_MAX {
            return Err(Errno::EINVAL);
        }
        info.stable = false;
        let inner = Arc::new(Inner {
            provider,
            info,
            caps,
            nodes: Mutex::new(HashMap::new()),
        });
        let reply = inner.provider.call(Request::Root).await?;
        let root = inner.node(reply)?;
        if root.file_type() != FileType::Directory {
            return Err(Errno::ENOTDIR);
        }
        Ok(Arc::new(Self { inner, root }))
    }

    pub fn capabilities(&self) -> Capabilities {
        self.inner.caps
    }
}

#[async_trait(?Send)]
impl Filesystem for WebFs {
    fn limits_file_size(&self) -> bool {
        true
    }

    fn info(&self) -> FsInfo {
        self.inner.info.clone()
    }

    fn root(&self) -> Arc<dyn Node> {
        self.root.clone()
    }

    async fn statfs(&self) -> FsResult<StatFs> {
        match self.inner.provider.call(Request::StatFs).await? {
            Reply::StatFs(stat) => Ok(stat),
            reply => self.inner.bad(reply),
        }
    }

    async fn sync(&self) -> FsResult<()> {
        self.inner.done(Request::SyncFs).await
    }
}

impl Inner {
    fn writable(&self) -> FsResult<()> {
        if self.info.read_only {
            Err(Errno::EROFS)
        } else {
            Ok(())
        }
    }

    fn supports(&self, supported: bool) -> FsResult<()> {
        if supported {
            Ok(())
        } else {
            Err(Errno::EOPNOTSUPP)
        }
    }

    fn name(&self, name: &[u8]) -> FsResult<()> {
        if name.len() > self.info.name_max {
            return Err(Errno::ENAMETOOLONG);
        }
        if name.is_empty()
            || name == b"."
            || name == b".."
            || name.contains(&0)
            || name.contains(&b'/')
        {
            return Err(Errno::EINVAL);
        }
        Ok(())
    }

    fn creation(&self, name: Option<&[u8]>, new: &NewNode<'_>) -> FsResult<Create> {
        self.writable()?;
        if let Some(name) = name {
            self.name(name)?;
        }
        match new.kind.file_type() {
            FileType::Regular | FileType::Directory => {}
            FileType::Symlink => self.supports(self.caps.symlinks)?,
            _ => self.supports(self.caps.special_files)?,
        }
        Ok(Create::from_new(new))
    }

    // A bad reply must not leak a lease even when its tag is unexpected.
    fn bad<T>(&self, reply: Reply) -> FsResult<T> {
        self.release_reply(reply);
        Err(Errno::EIO)
    }

    fn release_reply(&self, reply: Reply) {
        match reply {
            Reply::Node(node) => self.provider.release(Resource::Node(node.id)),
            Reply::Open(handle) => self.provider.release(Resource::Handle(handle)),
            Reply::Created(node, handle) => {
                self.provider.release(Resource::Handle(handle));
                self.provider.release(Resource::Node(node.id));
            }
            _ => {}
        }
    }

    async fn done(&self, request: Request) -> FsResult<()> {
        match self.provider.call(request).await? {
            Reply::Done => Ok(()),
            reply => self.bad(reply),
        }
    }

    async fn bytes(&self, request: Request) -> FsResult<Vec<u8>> {
        match self.provider.call(request).await? {
            Reply::Bytes(bytes) => Ok(bytes),
            reply => self.bad(reply),
        }
    }

    fn node(self: &Arc<Self>, reply: Reply) -> FsResult<Arc<dyn Node>> {
        let Reply::Node(reference) = reply else {
            return self.bad(reply);
        };
        let mut nodes = self.nodes.lock().unwrap();
        if let Some(node) = nodes.get(&reference.ino).and_then(Weak::upgrade) {
            drop(nodes);
            self.provider.release(Resource::Node(reference.id));
            if node.reference.generation != reference.generation
                || node.reference.file_type != reference.file_type
            {
                return Err(Errno::EIO);
            }
            return Ok(node);
        }
        let node = Arc::new(WebNode {
            inner: self.clone(),
            reference,
        });
        nodes.insert(reference.ino, Arc::downgrade(&node));
        Ok(node)
    }

    fn opened(self: &Arc<Self>, node: Arc<dyn Node>, id: ObjectId) -> Box<dyn Handle> {
        Box::new(WebHandle {
            inner: self.clone(),
            id,
            _node: node,
        })
    }

    fn created(self: &Arc<Self>, reply: Reply) -> FsResult<(Arc<dyn Node>, Box<dyn Handle>)> {
        let Reply::Created(reference, id) = reply else {
            return self.bad(reply);
        };
        match self.node(Reply::Node(reference)) {
            Ok(node) => Ok((node.clone(), self.opened(node, id))),
            Err(error) => {
                self.provider.release(Resource::Handle(id));
                Err(error)
            }
        }
    }
}

struct WebNode {
    inner: Arc<Inner>,
    reference: NodeRef,
}

impl WebNode {
    fn id(&self) -> ObjectId {
        self.reference.id
    }

    fn same_provider(&self, node: &Arc<dyn Node>) -> FsResult<ObjectId> {
        let other = node.as_any().downcast_ref::<Self>().ok_or(Errno::EXDEV)?;
        if !Arc::ptr_eq(&self.inner, &other.inner) {
            return Err(Errno::EXDEV);
        }
        Ok(other.id())
    }

    fn attr(&self, reply: Reply) -> FsResult<Attr> {
        match reply {
            Reply::Attr(attr) if attr.ino == self.ino() && attr.file_type == self.file_type() => {
                Ok(attr)
            }
            reply => self.inner.bad(reply),
        }
    }
}

impl Drop for WebNode {
    fn drop(&mut self) {
        // Remove only our weak entry; a new reference might already occupy it.
        let mut nodes = self.inner.nodes.lock().unwrap();
        if nodes
            .get(&self.ino())
            .is_some_and(|node| std::ptr::eq(node.as_ptr(), self))
        {
            nodes.remove(&self.ino());
        }
        drop(nodes);
        self.inner.provider.release(Resource::Node(self.id()));
    }
}

#[async_trait(?Send)]
impl Node for WebNode {
    fn ino(&self) -> u64 {
        self.reference.ino
    }

    fn file_type(&self) -> FileType {
        self.reference.file_type
    }

    fn generation(&self) -> u32 {
        self.reference.generation
    }

    fn acl_version(&self) -> u64 {
        self.inner.provider.acl_version()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn getattr(&self) -> FsResult<Attr> {
        self.attr(
            self.inner
                .provider
                .call(Request::GetAttr(self.id()))
                .await?,
        )
    }

    async fn setattr(&self, attr: &SetAttr) -> FsResult<Attr> {
        self.inner.writable()?;
        self.attr(
            self.inner
                .provider
                .call(Request::SetAttr(self.id(), attr.clone()))
                .await?,
        )
    }

    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        self.inner.name(name)?;
        self.inner.node(
            self.inner
                .provider
                .call(Request::Lookup(self.id(), name.to_vec()))
                .await?,
        )
    }

    async fn create(&self, name: &[u8], new: &NewNode<'_>) -> FsResult<Arc<dyn Node>> {
        let new = self.inner.creation(Some(name), new)?;
        self.inner.node(
            self.inner
                .provider
                .call(Request::Create(self.id(), name.to_vec(), new))
                .await?,
        )
    }

    async fn create_open(
        &self,
        name: &[u8],
        new: &NewNode<'_>,
        options: OpenOptions,
    ) -> FsResult<(Arc<dyn Node>, Box<dyn Handle>)> {
        let new = self.inner.creation(Some(name), new)?;
        self.inner.created(
            self.inner
                .provider
                .call(Request::CreateOpen(self.id(), name.to_vec(), new, options))
                .await?,
        )
    }

    async fn create_unnamed(
        &self,
        new: &NewNode<'_>,
        options: OpenOptions,
    ) -> FsResult<(Arc<dyn Node>, Box<dyn Handle>)> {
        let new = self.inner.creation(None, new)?;
        self.inner.supports(self.inner.caps.unnamed_files)?;
        self.inner.created(
            self.inner
                .provider
                .call(Request::CreateUnnamed(self.id(), new, options))
                .await?,
        )
    }

    async fn link(&self, name: &[u8], target: &Arc<dyn Node>) -> FsResult<()> {
        self.inner.writable()?;
        self.inner.name(name)?;
        let target = self.same_provider(target)?;
        self.inner.supports(self.inner.caps.hard_links)?;
        self.inner
            .done(Request::Link(self.id(), name.to_vec(), target))
            .await
    }

    async fn unlink(&self, name: &[u8]) -> FsResult<()> {
        self.inner.writable()?;
        self.inner.name(name)?;
        self.inner
            .done(Request::Unlink(self.id(), name.to_vec()))
            .await
    }

    async fn rmdir(&self, name: &[u8]) -> FsResult<()> {
        self.inner.writable()?;
        self.inner.name(name)?;
        self.inner
            .done(Request::Rmdir(self.id(), name.to_vec()))
            .await
    }

    async fn rename(
        &self,
        name: &[u8],
        new_dir: &Arc<dyn Node>,
        new_name: &[u8],
        mode: RenameMode,
    ) -> FsResult<()> {
        self.inner.writable()?;
        self.inner.name(name)?;
        self.inner.name(new_name)?;
        let new_dir = self.same_provider(new_dir)?;
        if match mode {
            RenameMode::Replace => false,
            RenameMode::NoReplace => !self.inner.caps.rename_noreplace,
            RenameMode::Exchange => !self.inner.caps.rename_exchange,
            RenameMode::Whiteout => true,
        } {
            return Err(Errno::EINVAL);
        }
        self.inner
            .done(Request::Rename(
                self.id(),
                name.to_vec(),
                new_dir,
                new_name.to_vec(),
                mode,
            ))
            .await
    }

    async fn readlink(&self) -> FsResult<Vec<u8>> {
        self.inner.bytes(Request::ReadLink(self.id())).await
    }

    async fn open(&self, options: OpenOptions) -> FsResult<Box<dyn Handle>> {
        if options.write {
            self.inner.writable()?;
        }
        match self
            .inner
            .provider
            .call(Request::Open(self.id(), options))
            .await?
        {
            Reply::Open(id) => {
                let node = self
                    .inner
                    .nodes
                    .lock()
                    .unwrap()
                    .get(&self.ino())
                    .and_then(Weak::upgrade)
                    .ok_or(Errno::EIO)?;
                Ok(self.inner.opened(node, id))
            }
            reply => self.inner.bad(reply),
        }
    }

    async fn getxattr(&self, name: &[u8]) -> FsResult<Vec<u8>> {
        self.inner.supports(self.inner.caps.xattrs)?;
        self.inner
            .bytes(Request::GetXattr(self.id(), name.to_vec()))
            .await
    }

    async fn setxattr(&self, name: &[u8], value: &[u8], how: XattrSet) -> FsResult<()> {
        self.inner.writable()?;
        self.inner.supports(self.inner.caps.xattrs)?;
        self.inner
            .done(Request::SetXattr(
                self.id(),
                name.to_vec(),
                value.to_vec(),
                how,
            ))
            .await
    }

    async fn listxattr(&self) -> FsResult<Vec<Vec<u8>>> {
        self.inner.supports(self.inner.caps.xattrs)?;
        match self
            .inner
            .provider
            .call(Request::ListXattr(self.id()))
            .await?
        {
            Reply::Names(names) => Ok(names),
            reply => self.inner.bad(reply),
        }
    }

    async fn removexattr(&self, name: &[u8]) -> FsResult<()> {
        self.inner.writable()?;
        self.inner.supports(self.inner.caps.xattrs)?;
        self.inner
            .done(Request::RemoveXattr(self.id(), name.to_vec()))
            .await
    }
}

struct WebHandle {
    inner: Arc<Inner>,
    id: ObjectId,
    _node: Arc<dyn Node>,
}

impl Drop for WebHandle {
    fn drop(&mut self) {
        // release precedes the inode lease, as __fput precedes iput.
        self.inner.provider.release(Resource::Handle(self.id));
    }
}

#[async_trait(?Send)]
impl Handle for WebHandle {
    async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let len = buf.len().min(MAX_IO);
        let bytes = self
            .inner
            .bytes(Request::Read(self.id, offset, len))
            .await?;
        if bytes.len() > len {
            return Err(Errno::EIO);
        }
        buf[..bytes.len()].copy_from_slice(&bytes);
        Ok(bytes.len())
    }

    async fn write_at(&self, data: &[u8], offset: u64) -> FsResult<usize> {
        self.inner.writable()?;
        let data = &data[..data.len().min(MAX_IO)];
        match self
            .inner
            .provider
            .call(Request::Write(self.id, offset, data.to_vec()))
            .await?
        {
            Reply::Count(n) if n <= data.len() => Ok(n),
            reply => self.inner.bad(reply),
        }
    }

    async fn append(&self, data: &[u8]) -> FsResult<(u64, usize)> {
        self.inner.writable()?;
        self.inner.supports(self.inner.caps.atomic_append)?;
        let data = &data[..data.len().min(MAX_IO)];
        match self
            .inner
            .provider
            .call(Request::Append(self.id, data.to_vec()))
            .await?
        {
            Reply::Appended(at, n) if n <= data.len() && at.checked_add(n as u64).is_some() => {
                Ok((at, n))
            }
            reply => self.inner.bad(reply),
        }
    }

    async fn sync(&self, data_only: bool) -> FsResult<()> {
        self.inner.done(Request::Sync(self.id, data_only)).await
    }

    async fn set_size(&self, size: u64) -> FsResult<()> {
        self.inner.writable()?;
        self.inner.done(Request::SetSize(self.id, size)).await
    }

    async fn allocate(&self, mode: AllocMode, offset: u64, len: u64) -> FsResult<()> {
        self.inner.writable()?;
        self.inner.supports(self.inner.caps.allocation)?;
        self.inner
            .done(Request::Allocate(self.id, mode, offset, len))
            .await
    }

    async fn seek_hole(&self, offset: u64, size: u64, which: SeekHole) -> FsResult<u64> {
        match self
            .inner
            .provider
            .call(Request::SeekHole(self.id, offset, size, which))
            .await?
        {
            Reply::Offset(at) if at >= offset && at <= size => Ok(at),
            reply => self.inner.bad(reply),
        }
    }

    async fn readdir(&self, mut cookie: u64, sink: &mut dyn DirSink) -> FsResult<()> {
        loop {
            let entries = match self
                .inner
                .provider
                .call(Request::ReadDir(self.id, cookie, READDIR_BATCH))
                .await?
            {
                Reply::Entries(entries) if entries.len() <= READDIR_BATCH => entries,
                reply => return self.inner.bad(reply),
            };
            if entries.is_empty() {
                return Ok(());
            }
            // Validate the whole batch before publishing any entry to the caller.
            let mut next = cookie;
            for entry in &entries {
                if self.inner.name(&entry.name).is_err()
                    || entry.next == 0
                    || entry.next > MAX_DIR_COOKIE
                    || entry.next == next
                {
                    return Err(Errno::EIO);
                }
                next = entry.next;
            }
            for entry in entries {
                if !sink.push(DirEntry {
                    name: &entry.name,
                    ino: entry.ino,
                    file_type: entry.file_type,
                    next: entry.next,
                }) {
                    return Ok(());
                }
                cookie = entry.next;
            }
        }
    }
}

#[cfg(test)]
mod tests;
