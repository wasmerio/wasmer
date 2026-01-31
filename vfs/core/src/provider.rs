//! Filesystem provider abstraction and capability flags.
//!
//! Terminology (Linux-like):
//! - [`FsProvider`]: filesystem type/driver (creates filesystem instances).
//! - [`Fs`]: a mounted filesystem instance (superblock-like).
//! - `Mount`: the binding of an [`Fs`] into the VFS namespace (implemented later in `mount.rs`).

use crate::path_types::{VfsPath, VfsPathBuf};
use crate::traits_async::{FsAsync, FsHandleAsync, FsNodeAsync, FsProviderAsync};
use crate::traits_sync::{FsHandleSync, FsNodeSync, FsProviderSync, FsSync};
use crate::{VfsError, VfsErrorKind, VfsResult};
use bitflags::bitflags;
use std::any::Any;
use std::borrow::Cow;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct FsProviderCapabilities: u64 {
        const WATCH = 1 << 0;
        const HARDLINK = 1 << 1;
        const SYMLINK = 1 << 2;
        const RENAME_ATOMIC = 1 << 3;
        const ATOMIC_RENAME = 1 << 3;
        const SPARSE = 1 << 4;
        const XATTR = 1 << 5;
        const FILE_LOCKS = 1 << 6;
        const ATOMIC_O_TMPFILE = 1 << 7;
        const O_TMPFILE = 1 << 7;
        const CASE_SENSITIVE = 1 << 8;
        const CASE_PRESERVING = 1 << 9;
        const UNIX_PERMISSIONS = 1 << 10;
        const UTIMENS = 1 << 11;
        const STABLE_INODES = 1 << 12;
        const SEEK = 1 << 13;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct MountFlags: u64 {
        const READ_ONLY = 1 << 0;
        const NO_EXEC = 1 << 1;
        const NO_SUID = 1 << 2;
        const NODEV = 1 << 3;
        const NO_DEV = 1 << 3;
    }
}

/// Registry-visible provider name (e.g. "mem", "host", "overlay").
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProviderName(pub Cow<'static, str>);

/// Optional filesystem instance id (used for logging or watch routing).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FsInstanceId(pub u64);

/// Provider-specific configuration object for mount.
///
/// This is intentionally type-erased so a registry can store heterogeneous providers.
/// Providers can downcast to their concrete config type via [`ProviderConfig::as_any`].
pub trait ProviderConfig: Send + Sync + fmt::Debug + 'static {
    fn as_any(&self) -> &dyn Any;
}

impl<T> ProviderConfig for T
where
    T: Any + Send + Sync + fmt::Debug + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub type ProviderConfigBox = Box<dyn ProviderConfig>;

pub fn config_downcast_ref<T: 'static>(cfg: &dyn ProviderConfig) -> Option<&T> {
    cfg.as_any().downcast_ref::<T>()
}

#[derive(Debug)]
pub struct MountRequest<'a> {
    /// Where this mount will be attached in the VFS namespace.
    ///
    /// This is VFS-level input (used for validation/logging); it does not imply the backend
    /// operates on global absolute paths.
    pub target_path: &'a VfsPath,
    pub flags: MountFlags,
    pub config: &'a dyn ProviderConfig,
}

pub use crate::traits_sync::FsProviderSync as FsProvider;

pub trait VfsRuntime: Send + Sync {
    fn spawn_blocking_boxed(
        &self,
        f: Box<dyn FnOnce() -> Box<dyn Any + Send> + Send>,
    ) -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send>>;

    fn block_on_boxed<'a>(
        &'a self,
        fut: Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send + 'a>>,
    ) -> Box<dyn Any + Send>;
}

pub trait VfsRuntimeExt {
    fn spawn_blocking<F, R>(&self, f: F) -> Pin<Box<dyn Future<Output = R> + Send>>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static;

    fn block_on<'a, F: Future + Send + 'a>(&'a self, fut: F) -> F::Output
    where
        F::Output: Send + 'static;
}

impl<T> VfsRuntimeExt for T
where
    T: VfsRuntime + ?Sized,
{
    fn spawn_blocking<F, R>(&self, f: F) -> Pin<Box<dyn Future<Output = R> + Send>>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let fut = self.spawn_blocking_boxed(Box::new(move || {
            Box::new(f()) as Box<dyn Any + Send>
        }));
        Box::pin(async move {
            let value = fut.await;
            *value
                .downcast::<R>()
                .expect("vfs_runtime.spawn_blocking type mismatch")
        })
    }

    fn block_on<'a, F: Future + Send + 'a>(&'a self, fut: F) -> F::Output
    where
        F::Output: Send + 'static,
    {
        let fut = Box::pin(async move { Box::new(fut.await) as Box<dyn Any + Send> });
        let output = self.block_on_boxed(fut);
        *output
            .downcast::<F::Output>()
            .expect("vfs_runtime.block_on type mismatch")
    }
}

pub struct AsyncAdapter<T> {
    pub inner: T,
    pub rt: Arc<dyn VfsRuntime>,
}

pub struct SyncAdapter<T> {
    pub inner: T,
    pub rt: Arc<dyn VfsRuntime>,
}

#[derive(Clone)]
pub struct AsyncProviderFromSync {
    inner: Arc<dyn FsProviderSync>,
    rt: Arc<dyn VfsRuntime>,
}

impl AsyncProviderFromSync {
    pub fn new(inner: Arc<dyn FsProviderSync>, rt: Arc<dyn VfsRuntime>) -> Self {
        Self { inner, rt }
    }
}

#[async_trait::async_trait]
impl FsProviderAsync for AsyncProviderFromSync {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn capabilities(&self) -> FsProviderCapabilities {
        self.inner.capabilities()
    }

    fn validate_config(&self, config: &dyn ProviderConfig) -> VfsResult<()> {
        self.inner.validate_config(config)
    }

    async fn mount(&self, req: MountRequest<'_>) -> VfsResult<Arc<dyn FsAsync>> {
        // Mount requests borrow config/path; keep this inline to avoid spawning with
        // non-'static references.
        let fs = self.inner.mount(req)?;
        Ok(Arc::new(AsyncFsFromSync::new(fs, self.rt.clone())) as Arc<dyn FsAsync>)
    }
}

#[derive(Clone)]
pub struct AsyncFsFromSync {
    inner: Arc<dyn FsSync>,
    rt: Arc<dyn VfsRuntime>,
}

impl AsyncFsFromSync {
    pub fn new(inner: Arc<dyn FsSync>, rt: Arc<dyn VfsRuntime>) -> Self {
        Self { inner, rt }
    }

    fn wrap_node(&self, node: Arc<dyn FsNodeSync>) -> Arc<dyn FsNodeAsync> {
        Arc::new(AsyncNodeFromSync::new(node, self.rt.clone()))
    }
}

#[async_trait::async_trait]
impl FsAsync for AsyncFsFromSync {
    fn provider_name(&self) -> &'static str {
        self.inner.provider_name()
    }

    fn capabilities(&self) -> crate::VfsCapabilities {
        self.inner.capabilities()
    }

    async fn root(&self) -> VfsResult<Arc<dyn FsNodeAsync>> {
        let inner = self.inner.clone();
        let node = self.rt.spawn_blocking(move || inner.root()).await?;
        Ok(self.wrap_node(node))
    }

    async fn node_by_inode(
        &self,
        inode: crate::BackendInodeId,
    ) -> VfsResult<Option<Arc<dyn FsNodeAsync>>> {
        let inner = self.inner.clone();
        let node = self
            .rt
            .spawn_blocking(move || Ok(inner.node_by_inode(inode)))
            .await?;
        Ok(node.map(|node| self.wrap_node(node)))
    }
}

#[derive(Clone)]
pub struct AsyncNodeFromSync {
    inner: Arc<dyn FsNodeSync>,
    rt: Arc<dyn VfsRuntime>,
}

impl AsyncNodeFromSync {
    pub fn new(inner: Arc<dyn FsNodeSync>, rt: Arc<dyn VfsRuntime>) -> Self {
        Self { inner, rt }
    }

    fn wrap_node(&self, node: Arc<dyn FsNodeSync>) -> Arc<dyn FsNodeAsync> {
        Arc::new(Self::new(node, self.rt.clone()))
    }

    fn wrap_handle(&self, handle: Arc<dyn FsHandleSync>) -> Arc<dyn FsHandleAsync> {
        Arc::new(AsyncHandleFromSync::new(handle, self.rt.clone()))
    }

    fn try_sync_parent(node: &dyn FsNodeAsync) -> Option<Arc<dyn FsNodeSync>> {
        node.as_any()
            .downcast_ref::<AsyncNodeFromSync>()
            .map(|wrapper| wrapper.inner.clone())
    }
}

#[async_trait::async_trait]
impl FsNodeAsync for AsyncNodeFromSync {
    fn inode(&self) -> crate::BackendInodeId {
        self.inner.inode()
    }

    fn file_type(&self) -> crate::VfsFileType {
        self.inner.file_type()
    }

    async fn metadata(&self) -> VfsResult<crate::VfsMetadata> {
        let inner = self.inner.clone();
        self.rt.spawn_blocking(move || inner.metadata()).await?
    }

    async fn set_metadata(&self, set: crate::VfsSetMetadata) -> VfsResult<()> {
        let inner = self.inner.clone();
        self.rt.spawn_blocking(move || inner.set_metadata(set)).await?
    }

    async fn lookup(&self, name: &crate::VfsName) -> VfsResult<Arc<dyn FsNodeAsync>> {
        let inner = self.inner.clone();
        let name = name.as_bytes().to_vec();
        let node = self
            .rt
            .spawn_blocking(move || {
                let name = crate::VfsName::new(&name)?;
                inner.lookup(&name)
            })
            .await?;
        Ok(self.wrap_node(node))
    }

    async fn create_file(
        &self,
        name: &crate::VfsName,
        opts: crate::node::CreateFile,
    ) -> VfsResult<Arc<dyn FsNodeAsync>> {
        let inner = self.inner.clone();
        let name = name.as_bytes().to_vec();
        let node = self
            .rt
            .spawn_blocking(move || {
                let name = crate::VfsName::new(&name)?;
                inner.create_file(&name, opts)
            })
            .await?;
        Ok(self.wrap_node(node))
    }

    async fn mkdir(
        &self,
        name: &crate::VfsName,
        opts: crate::node::MkdirOptions,
    ) -> VfsResult<Arc<dyn FsNodeAsync>> {
        let inner = self.inner.clone();
        let name = name.as_bytes().to_vec();
        let node = self
            .rt
            .spawn_blocking(move || {
                let name = crate::VfsName::new(&name)?;
                inner.mkdir(&name, opts)
            })
            .await?;
        Ok(self.wrap_node(node))
    }

    async fn unlink(
        &self,
        name: &crate::VfsName,
        opts: crate::node::UnlinkOptions,
    ) -> VfsResult<()> {
        let inner = self.inner.clone();
        let name = name.as_bytes().to_vec();
        self.rt
            .spawn_blocking(move || {
                let name = crate::VfsName::new(&name)?;
                inner.unlink(&name, opts)
            })
            .await?
    }

    async fn rmdir(&self, name: &crate::VfsName) -> VfsResult<()> {
        let inner = self.inner.clone();
        let name = name.as_bytes().to_vec();
        self.rt
            .spawn_blocking(move || {
                let name = crate::VfsName::new(&name)?;
                inner.rmdir(&name)
            })
            .await?
    }

    async fn read_dir(
        &self,
        cursor: Option<crate::node::DirCursor>,
        max: usize,
    ) -> VfsResult<crate::node::ReadDirBatch> {
        let inner = self.inner.clone();
        self.rt
            .spawn_blocking(move || inner.read_dir(cursor, max))
            .await?
    }

    async fn rename(
        &self,
        old_name: &crate::VfsName,
        new_parent: &dyn FsNodeAsync,
        new_name: &crate::VfsName,
        opts: crate::node::RenameOptions,
    ) -> VfsResult<()> {
        let Some(sync_parent) = Self::try_sync_parent(new_parent) else {
            return Err(VfsError::new(VfsErrorKind::CrossDevice, "adapter.rename.cross_device"));
        };
        let inner = self.inner.clone();
        let old_name = old_name.as_bytes().to_vec();
        let new_name = new_name.as_bytes().to_vec();
        self.rt
            .spawn_blocking(move || {
                let old_name = crate::VfsName::new(&old_name)?;
                let new_name = crate::VfsName::new(&new_name)?;
                inner.rename(&old_name, sync_parent.as_ref(), &new_name, opts)
            })
            .await?
    }

    async fn link(&self, existing: &dyn FsNodeAsync, new_name: &crate::VfsName) -> VfsResult<()> {
        let Some(sync_existing) = Self::try_sync_parent(existing) else {
            return Err(VfsError::new(VfsErrorKind::CrossDevice, "adapter.link.cross_device"));
        };
        let inner = self.inner.clone();
        let new_name = new_name.as_bytes().to_vec();
        self.rt
            .spawn_blocking(move || {
                let new_name = crate::VfsName::new(&new_name)?;
                inner.link(sync_existing.as_ref(), &new_name)
            })
            .await?
    }

    async fn symlink(&self, new_name: &crate::VfsName, target: &crate::VfsPath) -> VfsResult<()> {
        let inner = self.inner.clone();
        let new_name = new_name.as_bytes().to_vec();
        let target = target.to_vec();
        self.rt
            .spawn_blocking(move || {
                let new_name = crate::VfsName::new(&new_name)?;
                let target = crate::VfsPathBuf::from_bytes(target);
                inner.symlink(&new_name, target.as_path())
            })
            .await?
    }

    async fn readlink(&self) -> VfsResult<crate::VfsPathBuf> {
        let inner = self.inner.clone();
        self.rt.spawn_blocking(move || inner.readlink()).await?
    }

    async fn open(&self, opts: crate::flags::OpenOptions) -> VfsResult<Arc<dyn FsHandleAsync>> {
        let inner = self.inner.clone();
        let handle = self.rt.spawn_blocking(move || inner.open(opts)).await?;
        Ok(self.wrap_handle(handle))
    }
}

#[derive(Clone)]
pub struct AsyncHandleFromSync {
    inner: Arc<dyn FsHandleSync>,
    rt: Arc<dyn VfsRuntime>,
}

impl AsyncHandleFromSync {
    pub fn new(inner: Arc<dyn FsHandleSync>, rt: Arc<dyn VfsRuntime>) -> Self {
        Self { inner, rt }
    }
}

#[async_trait::async_trait]
impl FsHandleAsync for AsyncHandleFromSync {
    async fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let inner = self.inner.clone();
        let mut temp = vec![0u8; buf.len()];
        let (read, data) = self
            .rt
            .spawn_blocking(move || {
                let read = inner.read_at(offset, &mut temp)?;
                Ok((read, temp))
            })
            .await?;
        buf[..read].copy_from_slice(&data[..read]);
        Ok(read)
    }

    async fn write_at(&self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        let inner = self.inner.clone();
        let data = buf.to_vec();
        self.rt
            .spawn_blocking(move || inner.write_at(offset, &data))
            .await?
    }

    async fn flush(&self) -> VfsResult<()> {
        let inner = self.inner.clone();
        self.rt.spawn_blocking(move || inner.flush()).await?
    }

    async fn fsync(&self) -> VfsResult<()> {
        let inner = self.inner.clone();
        self.rt.spawn_blocking(move || inner.fsync()).await?
    }

    async fn get_metadata(&self) -> VfsResult<crate::VfsMetadata> {
        let inner = self.inner.clone();
        self.rt.spawn_blocking(move || inner.get_metadata()).await?
    }

    async fn set_len(&self, len: u64) -> VfsResult<()> {
        let inner = self.inner.clone();
        self.rt.spawn_blocking(move || inner.set_len(len)).await?
    }

    async fn len(&self) -> VfsResult<u64> {
        let inner = self.inner.clone();
        self.rt.spawn_blocking(move || inner.len()).await?
    }

    async fn append(&self, buf: &[u8]) -> VfsResult<Option<usize>> {
        let inner = self.inner.clone();
        let data = buf.to_vec();
        self.rt
            .spawn_blocking(move || inner.append(&data))
            .await?
    }

    async fn dup(&self) -> VfsResult<Option<Arc<dyn FsHandleAsync>>> {
        let inner = self.inner.clone();
        let handle = self.rt.spawn_blocking(move || inner.dup()).await?;
        Ok(handle.map(|handle| Arc::new(AsyncHandleFromSync::new(handle, self.rt.clone())) as _))
    }

    fn is_seekable(&self) -> bool {
        self.inner.is_seekable()
    }
}

#[derive(Clone)]
pub struct SyncProviderFromAsync {
    inner: Arc<dyn FsProviderAsync>,
    rt: Arc<dyn VfsRuntime>,
}

impl SyncProviderFromAsync {
    pub fn new(inner: Arc<dyn FsProviderAsync>, rt: Arc<dyn VfsRuntime>) -> Self {
        Self { inner, rt }
    }
}

impl FsProviderSync for SyncProviderFromAsync {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn capabilities(&self) -> FsProviderCapabilities {
        self.inner.capabilities()
    }

    fn validate_config(&self, config: &dyn ProviderConfig) -> VfsResult<()> {
        self.inner.validate_config(config)
    }

    fn mount(&self, req: MountRequest<'_>) -> VfsResult<Arc<dyn FsSync>> {
        let inner = self.inner.clone();
        let rt = self.rt.clone();
        let fs = rt.block_on(inner.mount(req))?;
        Ok(Arc::new(SyncFsFromAsync::new(fs, rt)) as Arc<dyn FsSync>)
    }
}

#[derive(Clone)]
pub struct SyncFsFromAsync {
    inner: Arc<dyn FsAsync>,
    rt: Arc<dyn VfsRuntime>,
}

impl SyncFsFromAsync {
    pub fn new(inner: Arc<dyn FsAsync>, rt: Arc<dyn VfsRuntime>) -> Self {
        Self { inner, rt }
    }

    fn wrap_node(&self, node: Arc<dyn FsNodeAsync>) -> Arc<dyn FsNodeSync> {
        Arc::new(SyncNodeFromAsync::new(node, self.rt.clone()))
    }
}

impl FsSync for SyncFsFromAsync {
    fn provider_name(&self) -> &'static str {
        self.inner.provider_name()
    }

    fn capabilities(&self) -> crate::VfsCapabilities {
        self.inner.capabilities()
    }

    fn root(&self) -> Arc<dyn FsNodeSync> {
        let node = self.rt.block_on(self.inner.root()).expect("root");
        self.wrap_node(node)
    }

    fn node_by_inode(&self, inode: crate::BackendInodeId) -> Option<Arc<dyn FsNodeSync>> {
        let node = self
            .rt
            .block_on(self.inner.node_by_inode(inode))
            .unwrap_or(None);
        node.map(|node| self.wrap_node(node))
    }
}

#[derive(Clone)]
pub struct SyncNodeFromAsync {
    inner: Arc<dyn FsNodeAsync>,
    rt: Arc<dyn VfsRuntime>,
}

impl SyncNodeFromAsync {
    pub fn new(inner: Arc<dyn FsNodeAsync>, rt: Arc<dyn VfsRuntime>) -> Self {
        Self { inner, rt }
    }

    fn wrap_node(&self, node: Arc<dyn FsNodeAsync>) -> Arc<dyn FsNodeSync> {
        Arc::new(Self::new(node, self.rt.clone()))
    }

    fn wrap_handle(&self, handle: Arc<dyn FsHandleAsync>) -> Arc<dyn FsHandleSync> {
        Arc::new(SyncHandleFromAsync::new(handle, self.rt.clone()))
    }

    fn try_async_parent(node: &dyn FsNodeSync) -> Option<Arc<dyn FsNodeAsync>> {
        node.as_any()
            .downcast_ref::<SyncNodeFromAsync>()
            .map(|wrapper| wrapper.inner.clone())
    }
}

impl FsNodeSync for SyncNodeFromAsync {
    fn inode(&self) -> crate::BackendInodeId {
        self.inner.inode()
    }

    fn file_type(&self) -> crate::VfsFileType {
        self.inner.file_type()
    }

    fn metadata(&self) -> VfsResult<crate::VfsMetadata> {
        self.rt.block_on(self.inner.metadata())
    }

    fn set_metadata(&self, set: crate::VfsSetMetadata) -> VfsResult<()> {
        self.rt.block_on(self.inner.set_metadata(set))
    }

    fn lookup(&self, name: &crate::VfsName) -> VfsResult<Arc<dyn FsNodeSync>> {
        let node = self.rt.block_on(self.inner.lookup(name))?;
        Ok(self.wrap_node(node))
    }

    fn create_file(
        &self,
        name: &crate::VfsName,
        opts: crate::node::CreateFile,
    ) -> VfsResult<Arc<dyn FsNodeSync>> {
        let node = self.rt.block_on(self.inner.create_file(name, opts))?;
        Ok(self.wrap_node(node))
    }

    fn mkdir(
        &self,
        name: &crate::VfsName,
        opts: crate::node::MkdirOptions,
    ) -> VfsResult<Arc<dyn FsNodeSync>> {
        let node = self.rt.block_on(self.inner.mkdir(name, opts))?;
        Ok(self.wrap_node(node))
    }

    fn unlink(&self, name: &crate::VfsName, opts: crate::node::UnlinkOptions) -> VfsResult<()> {
        self.rt.block_on(self.inner.unlink(name, opts))
    }

    fn rmdir(&self, name: &crate::VfsName) -> VfsResult<()> {
        self.rt.block_on(self.inner.rmdir(name))
    }

    fn read_dir(
        &self,
        cursor: Option<crate::node::DirCursor>,
        max: usize,
    ) -> VfsResult<crate::node::ReadDirBatch> {
        self.rt.block_on(self.inner.read_dir(cursor, max))
    }

    fn rename(
        &self,
        old_name: &crate::VfsName,
        new_parent: &dyn FsNodeSync,
        new_name: &crate::VfsName,
        opts: crate::node::RenameOptions,
    ) -> VfsResult<()> {
        let Some(async_parent) = Self::try_async_parent(new_parent) else {
            return Err(VfsError::new(VfsErrorKind::CrossDevice, "adapter.rename.cross_device"));
        };
        self.rt
            .block_on(self.inner.rename(old_name, async_parent.as_ref(), new_name, opts))
    }

    fn link(&self, existing: &dyn FsNodeSync, new_name: &crate::VfsName) -> VfsResult<()> {
        let Some(async_existing) = Self::try_async_parent(existing) else {
            return Err(VfsError::new(VfsErrorKind::CrossDevice, "adapter.link.cross_device"));
        };
        self.rt
            .block_on(self.inner.link(async_existing.as_ref(), new_name))
    }

    fn symlink(&self, new_name: &crate::VfsName, target: &crate::VfsPath) -> VfsResult<()> {
        self.rt.block_on(self.inner.symlink(new_name, target))
    }

    fn readlink(&self) -> VfsResult<crate::VfsPathBuf> {
        self.rt.block_on(self.inner.readlink())
    }

    fn open(&self, opts: crate::flags::OpenOptions) -> VfsResult<Arc<dyn FsHandleSync>> {
        let handle = self.rt.block_on(self.inner.open(opts))?;
        Ok(self.wrap_handle(handle))
    }
}

#[derive(Clone)]
pub struct SyncHandleFromAsync {
    inner: Arc<dyn FsHandleAsync>,
    rt: Arc<dyn VfsRuntime>,
}

impl SyncHandleFromAsync {
    pub fn new(inner: Arc<dyn FsHandleAsync>, rt: Arc<dyn VfsRuntime>) -> Self {
        Self { inner, rt }
    }
}

impl FsHandleSync for SyncHandleFromAsync {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        self.rt.block_on(self.inner.read_at(offset, buf))
    }

    fn write_at(&self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        self.rt.block_on(self.inner.write_at(offset, buf))
    }

    fn flush(&self) -> VfsResult<()> {
        self.rt.block_on(self.inner.flush())
    }

    fn fsync(&self) -> VfsResult<()> {
        self.rt.block_on(self.inner.fsync())
    }

    fn get_metadata(&self) -> VfsResult<crate::VfsMetadata> {
        self.rt.block_on(self.inner.get_metadata())
    }

    fn set_len(&self, len: u64) -> VfsResult<()> {
        self.rt.block_on(self.inner.set_len(len))
    }

    fn len(&self) -> VfsResult<u64> {
        self.rt.block_on(self.inner.len())
    }

    fn append(&self, buf: &[u8]) -> VfsResult<Option<usize>> {
        self.rt.block_on(self.inner.append(buf))
    }

    fn dup(&self) -> VfsResult<Option<Arc<dyn FsHandleSync>>> {
        let handle = self.rt.block_on(self.inner.dup())?;
        Ok(handle.map(|handle| Arc::new(SyncHandleFromAsync::new(handle, self.rt.clone())) as _))
    }

    fn is_seekable(&self) -> bool {
        self.inner.is_seekable()
    }
}

/// Convenience config wrapper for providers that expect to receive a [`VfsPathBuf`].
#[derive(Debug, Clone)]
pub struct PathConfig {
    pub path: VfsPathBuf,
}

pub use crate::provider_registry::FsProviderRegistry;
