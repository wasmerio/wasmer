use std::borrow::Cow;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use deno_error::JsErrorBox;
use deno_runtime::deno_fs;
use deno_runtime::deno_permissions;
use node_resolver::PackageJsonResolverRc;
use node_resolver::errors::PackageJsonLoadError;
use sys_traits::FsMetadataValue as _;
use virtual_fs::{AsyncReadExt as _, AsyncSeekExt as _, AsyncWriteExt as _};

pub(crate) type DynFs = dyn virtual_fs::FileSystem + Send + Sync;

#[derive(Clone, Debug)]
pub(crate) struct FsBridge<T>(pub T);

#[derive(Debug)]
pub(crate) struct FsBridgeState<T> {
    fs: T,
    cwd: std::sync::Mutex<PathBuf>,
}

impl<T> FsBridgeState<T> {
    fn new(fs: T) -> Self {
        Self {
            fs,
            cwd: std::sync::Mutex::new(PathBuf::from("/")),
        }
    }
}

impl<T> FsBridge<Arc<FsBridgeState<T>>> {
    pub(crate) fn new(fs: T) -> Self {
        Self(Arc::new(FsBridgeState::new(fs)))
    }

    fn fs(&self) -> &T {
        &self.0.fs
    }

    fn cwd(&self) -> &std::sync::Mutex<PathBuf> {
        &self.0.cwd
    }
}

fn map_vfs_err(err: virtual_fs::FsError) -> io::Error {
    io::Error::from(err)
}

fn stat_from_meta(meta: virtual_fs::Metadata) -> deno_runtime::deno_io::fs::FsStat {
    let to_ms = |value: u64| Some(value / 1_000_000);
    deno_runtime::deno_io::fs::FsStat {
        is_file: meta.ft.is_file(),
        is_directory: meta.ft.is_dir(),
        is_symlink: meta.ft.is_symlink(),
        size: meta.len,
        mtime: to_ms(meta.modified),
        atime: to_ms(meta.accessed),
        birthtime: to_ms(meta.created),
        ctime: to_ms(meta.modified),
        dev: 0,
        ino: None,
        mode: 0,
        nlink: None,
        uid: 0,
        gid: 0,
        rdev: 0,
        blksize: 0,
        blocks: None,
        is_block_device: false,
        is_char_device: false,
        is_fifo: false,
        is_socket: false,
    }
}

#[derive(Clone, Debug)]
struct BridgeMetadata {
    meta: virtual_fs::Metadata,
}

impl sys_traits::FsMetadataValue for BridgeMetadata {
    fn file_type(&self) -> sys_traits::FileType {
        if self.meta.ft.is_dir() {
            sys_traits::FileType::Dir
        } else if self.meta.ft.is_file() {
            sys_traits::FileType::File
        } else if self.meta.ft.is_symlink() {
            sys_traits::FileType::Symlink
        } else {
            sys_traits::FileType::Unknown
        }
    }

    fn len(&self) -> u64 {
        self.meta.len
    }

    fn accessed(&self) -> io::Result<SystemTime> {
        Ok(UNIX_EPOCH + Duration::from_nanos(self.meta.accessed))
    }

    fn created(&self) -> io::Result<SystemTime> {
        Ok(UNIX_EPOCH + Duration::from_nanos(self.meta.created))
    }

    fn changed(&self) -> io::Result<SystemTime> {
        Ok(UNIX_EPOCH + Duration::from_nanos(self.meta.modified))
    }

    fn modified(&self) -> io::Result<SystemTime> {
        Ok(UNIX_EPOCH + Duration::from_nanos(self.meta.modified))
    }

    fn dev(&self) -> io::Result<u64> {
        Ok(0)
    }

    fn ino(&self) -> io::Result<u64> {
        Ok(0)
    }

    fn mode(&self) -> io::Result<u32> {
        Ok(0)
    }

    fn nlink(&self) -> io::Result<u64> {
        Ok(1)
    }

    fn uid(&self) -> io::Result<u32> {
        Ok(0)
    }

    fn gid(&self) -> io::Result<u32> {
        Ok(0)
    }

    fn rdev(&self) -> io::Result<u64> {
        Ok(0)
    }

    fn blksize(&self) -> io::Result<u64> {
        Ok(0)
    }

    fn blocks(&self) -> io::Result<u64> {
        Ok(0)
    }

    fn is_block_device(&self) -> io::Result<bool> {
        Ok(false)
    }

    fn is_char_device(&self) -> io::Result<bool> {
        Ok(false)
    }

    fn is_fifo(&self) -> io::Result<bool> {
        Ok(false)
    }

    fn is_socket(&self) -> io::Result<bool> {
        Ok(false)
    }

    fn file_attributes(&self) -> io::Result<u32> {
        Ok(0)
    }
}

#[derive(Debug)]
struct BridgeDirEntry {
    inner: virtual_fs::DirEntry,
}

impl sys_traits::FsDirEntry for BridgeDirEntry {
    type Metadata = BridgeMetadata;

    fn file_name(&self) -> Cow<'_, std::ffi::OsStr> {
        Cow::Owned(self.inner.file_name())
    }

    fn file_type(&self) -> io::Result<sys_traits::FileType> {
        let meta = self.inner.metadata().map_err(map_vfs_err)?;
        Ok(BridgeMetadata { meta }.file_type())
    }

    fn metadata(&self) -> io::Result<Self::Metadata> {
        let meta = self.inner.metadata().map_err(map_vfs_err)?;
        Ok(BridgeMetadata { meta })
    }

    fn path(&self) -> Cow<'_, Path> {
        Cow::Owned(self.inner.path())
    }
}

struct BridgeSyncFile<T> {
    fs: T,
    path: PathBuf,
    inner: std::sync::Mutex<Box<dyn virtual_fs::VirtualFile + Send + Sync + 'static>>,
}

impl<T> BridgeSyncFile<T> {
    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future))
    }
}

impl<T> std::io::Read for BridgeSyncFile<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        Self::block_on(async {
            let mut guard = self.inner.lock().unwrap();
            guard.read(buf).await
        })
    }
}

impl<T> std::io::Write for BridgeSyncFile<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Self::block_on(async {
            let mut guard = self.inner.lock().unwrap();
            guard.write(buf).await
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<T> std::io::Seek for BridgeSyncFile<T> {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        Self::block_on(async {
            let mut guard = self.inner.lock().unwrap();
            guard.seek(pos).await
        })
    }
}

impl<T> sys_traits::FsFileIsTerminal for BridgeSyncFile<T> {
    fn fs_file_is_terminal(&self) -> bool {
        false
    }
}

impl<T> sys_traits::FsFileLock for BridgeSyncFile<T> {
    fn fs_file_lock(&mut self, _mode: sys_traits::FsFileLockMode) -> io::Result<()> {
        Ok(())
    }

    fn fs_file_try_lock(&mut self, _mode: sys_traits::FsFileLockMode) -> io::Result<()> {
        Ok(())
    }

    fn fs_file_unlock(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<T> sys_traits::FsFileMetadata for BridgeSyncFile<T>
where
    T: virtual_fs::FileSystem,
{
    fn fs_file_metadata(&self) -> io::Result<sys_traits::boxed::BoxedFsMetadataValue> {
        let meta = self.fs.metadata(&self.path).map_err(map_vfs_err)?;
        Ok(sys_traits::boxed::BoxedFsMetadataValue(Box::new(
            BridgeMetadata { meta },
        )))
    }
}

impl<T> sys_traits::FsFileSetPermissions for BridgeSyncFile<T> {
    fn fs_file_set_permissions(&mut self, _mode: u32) -> io::Result<()> {
        Err(io::ErrorKind::PermissionDenied.into())
    }
}

impl<T> sys_traits::FsFileSetTimes for BridgeSyncFile<T> {
    fn fs_file_set_times(&mut self, _times: sys_traits::FsFileTimes) -> io::Result<()> {
        Err(io::ErrorKind::PermissionDenied.into())
    }
}

impl<T> sys_traits::FsFileSetLen for BridgeSyncFile<T> {
    fn fs_file_set_len(&mut self, _size: u64) -> io::Result<()> {
        Err(io::ErrorKind::PermissionDenied.into())
    }
}

impl<T> sys_traits::FsFileSyncAll for BridgeSyncFile<T> {
    fn fs_file_sync_all(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<T> sys_traits::FsFileSyncData for BridgeSyncFile<T> {
    fn fs_file_sync_data(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<T> sys_traits::FsFileAsRaw for BridgeSyncFile<T> {
    #[cfg(windows)]
    fn fs_file_as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        None
    }

    #[cfg(unix)]
    fn fs_file_as_raw_fd(&self) -> Option<std::os::fd::RawFd> {
        None
    }
}

impl<T> sys_traits::FsFile for BridgeSyncFile<T> {}

impl<T> sys_traits::BaseFsCanonicalize for FsBridge<Arc<FsBridgeState<T>>>
where
    T: virtual_fs::FileSystem + Clone,
{
    fn base_fs_canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(virtual_fs::host_fs::normalize_path(path))
    }
}

impl<T> sys_traits::BaseFsMetadata for FsBridge<Arc<FsBridgeState<T>>>
where
    T: virtual_fs::FileSystem + Clone,
{
    type Metadata = BridgeMetadata;

    fn base_fs_metadata(&self, path: &Path) -> io::Result<Self::Metadata> {
        let meta = self.fs().metadata(path).map_err(map_vfs_err)?;
        Ok(BridgeMetadata { meta })
    }

    fn base_fs_symlink_metadata(&self, path: &Path) -> io::Result<Self::Metadata> {
        let meta = self.fs().symlink_metadata(path).map_err(map_vfs_err)?;
        Ok(BridgeMetadata { meta })
    }
}

impl<T> sys_traits::BaseFsReadDir for FsBridge<Arc<FsBridgeState<T>>>
where
    T: virtual_fs::FileSystem + Clone,
{
    type ReadDirEntry = BridgeDirEntry;

    fn base_fs_read_dir(
        &self,
        path: &Path,
    ) -> io::Result<Box<dyn Iterator<Item = io::Result<Self::ReadDirEntry>>>> {
        let entries = self.fs().read_dir(path).map_err(map_vfs_err)?;
        let iter = entries
            .map(|entry| entry.map(|inner| BridgeDirEntry { inner }))
            .map(|entry| entry.map_err(map_vfs_err));
        Ok(Box::new(iter))
    }
}

impl<T> sys_traits::BaseFsRead for FsBridge<Arc<FsBridgeState<T>>>
where
    T: virtual_fs::FileSystem + Clone,
{
    fn base_fs_read(&self, path: &Path) -> io::Result<Cow<'static, [u8]>> {
        let mut file = self
            .fs()
            .new_open_options()
            .read(true)
            .open(path)
            .map_err(map_vfs_err)?;
        let mut buffer = Vec::new();
        BridgeSyncFile::<T>::block_on(async { file.read_to_end(&mut buffer).await })?;
        Ok(Cow::Owned(buffer))
    }
}

impl<T> sys_traits::BaseFsOpen for FsBridge<Arc<FsBridgeState<T>>>
where
    T: virtual_fs::FileSystem + Clone,
{
    type File = BridgeSyncFile<T>;

    fn base_fs_open(
        &self,
        path: &Path,
        options: &sys_traits::OpenOptions,
    ) -> io::Result<Self::File> {
        let mut opts = self.fs().new_open_options();
        opts.read(options.read)
            .write(options.write)
            .create(options.create)
            .truncate(options.truncate)
            .append(options.append)
            .create_new(options.create_new);
        let file = opts.open(path).map_err(map_vfs_err)?;
        Ok(BridgeSyncFile {
            fs: self.fs().clone(),
            path: path.to_path_buf(),
            inner: std::sync::Mutex::new(file),
        })
    }
}

impl<T> sys_traits::EnvCurrentDir for FsBridge<Arc<FsBridgeState<T>>>
where
    T: virtual_fs::FileSystem + Clone,
{
    fn env_current_dir(&self) -> io::Result<PathBuf> {
        Ok(PathBuf::from("/"))
    }
}

#[derive(Debug)]
struct BridgeFile<T> {
    fs: T,
    path: PathBuf,
    inner: tokio::sync::Mutex<Box<dyn virtual_fs::VirtualFile + Send + Sync + 'static>>,
}

impl<T> BridgeFile<T> {
    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future))
    }
}

#[async_trait::async_trait(?Send)]
impl<T> deno_runtime::deno_io::fs::File for BridgeFile<T>
where
    T: virtual_fs::FileSystem + Clone + Send + Sync + 'static,
{
    fn maybe_path(&self) -> Option<&Path> {
        Some(&self.path)
    }

    fn read_sync(self: Rc<Self>, buf: &mut [u8]) -> deno_runtime::deno_io::fs::FsResult<usize> {
        Self::block_on(async {
            let mut guard = self.inner.lock().await;
            guard
                .read(buf)
                .await
                .map_err(|err| io::Error::from(err).into())
        })
    }

    async fn read_byob(
        self: Rc<Self>,
        mut buf: deno_core::BufMutView,
    ) -> deno_runtime::deno_io::fs::FsResult<(usize, deno_core::BufMutView)> {
        let mut guard = self.inner.lock().await;
        let nread = guard
            .read(buf.as_mut())
            .await
            .map_err(|err| deno_runtime::deno_io::fs::FsError::from(io::Error::from(err)))?;
        Ok((nread, buf))
    }

    fn write_sync(self: Rc<Self>, buf: &[u8]) -> deno_runtime::deno_io::fs::FsResult<usize> {
        Self::block_on(async {
            let mut guard = self.inner.lock().await;
            guard
                .write(buf)
                .await
                .map_err(|err| io::Error::from(err).into())
        })
    }

    async fn write(
        self: Rc<Self>,
        buf: deno_core::BufView,
    ) -> deno_runtime::deno_io::fs::FsResult<deno_core::WriteOutcome> {
        let mut guard = self.inner.lock().await;
        let n = guard
            .write(buf.as_ref())
            .await
            .map_err(|err| deno_runtime::deno_io::fs::FsError::from(io::Error::from(err)))?;
        Ok(deno_core::WriteOutcome::Full { nwritten: n })
    }

    fn write_all_sync(self: Rc<Self>, buf: &[u8]) -> deno_runtime::deno_io::fs::FsResult<()> {
        Self::block_on(async {
            let mut guard = self.inner.lock().await;
            guard
                .write_all(buf)
                .await
                .map_err(|err| deno_runtime::deno_io::fs::FsError::from(io::Error::from(err)))
        })
    }

    async fn write_all(
        self: Rc<Self>,
        buf: deno_core::BufView,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        let mut guard = self.inner.lock().await;
        guard
            .write_all(buf.as_ref())
            .await
            .map_err(|err| deno_runtime::deno_io::fs::FsError::from(io::Error::from(err)))
    }

    fn read_all_sync(self: Rc<Self>) -> deno_runtime::deno_io::fs::FsResult<Cow<'static, [u8]>> {
        Self::block_on(async {
            let mut guard = self.inner.lock().await;
            let mut buffer = Vec::new();
            guard
                .read_to_end(&mut buffer)
                .await
                .map_err(|err| deno_runtime::deno_io::fs::FsError::from(io::Error::from(err)))?;
            Ok(Cow::Owned(buffer))
        })
    }

    async fn read_all_async(
        self: Rc<Self>,
    ) -> deno_runtime::deno_io::fs::FsResult<Cow<'static, [u8]>> {
        let mut guard = self.inner.lock().await;
        let mut buffer = Vec::new();
        guard
            .read_to_end(&mut buffer)
            .await
            .map_err(|err| deno_runtime::deno_io::fs::FsError::from(io::Error::from(err)))?;
        Ok(Cow::Owned(buffer))
    }

    fn chmod_sync(self: Rc<Self>, _pathmode: u32) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    async fn chmod_async(self: Rc<Self>, _mode: u32) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn chown_sync(
        self: Rc<Self>,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    async fn chown_async(
        self: Rc<Self>,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn seek_sync(self: Rc<Self>, pos: io::SeekFrom) -> deno_runtime::deno_io::fs::FsResult<u64> {
        Self::block_on(async {
            let mut guard = self.inner.lock().await;
            guard
                .seek(pos)
                .await
                .map_err(|err| deno_runtime::deno_io::fs::FsError::from(io::Error::from(err)))
        })
    }

    async fn seek_async(
        self: Rc<Self>,
        pos: io::SeekFrom,
    ) -> deno_runtime::deno_io::fs::FsResult<u64> {
        let mut guard = self.inner.lock().await;
        guard
            .seek(pos)
            .await
            .map_err(|err| deno_runtime::deno_io::fs::FsError::from(io::Error::from(err)))
    }

    fn datasync_sync(self: Rc<Self>) -> deno_runtime::deno_io::fs::FsResult<()> {
        Ok(())
    }

    async fn datasync_async(self: Rc<Self>) -> deno_runtime::deno_io::fs::FsResult<()> {
        Ok(())
    }

    fn sync_sync(self: Rc<Self>) -> deno_runtime::deno_io::fs::FsResult<()> {
        Ok(())
    }

    async fn sync_async(self: Rc<Self>) -> deno_runtime::deno_io::fs::FsResult<()> {
        Ok(())
    }

    fn stat_sync(
        self: Rc<Self>,
    ) -> deno_runtime::deno_io::fs::FsResult<deno_runtime::deno_io::fs::FsStat> {
        let meta = self
            .fs
            .metadata(&self.path)
            .map_err(|err| deno_runtime::deno_io::fs::FsError::from(io::Error::from(err)))?;
        Ok(stat_from_meta(meta))
    }

    async fn stat_async(
        self: Rc<Self>,
    ) -> deno_runtime::deno_io::fs::FsResult<deno_runtime::deno_io::fs::FsStat> {
        self.stat_sync()
    }

    fn lock_sync(self: Rc<Self>, _exclusive: bool) -> deno_runtime::deno_io::fs::FsResult<()> {
        Ok(())
    }

    async fn lock_async(
        self: Rc<Self>,
        _exclusive: bool,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Ok(())
    }

    fn unlock_sync(self: Rc<Self>) -> deno_runtime::deno_io::fs::FsResult<()> {
        Ok(())
    }

    async fn unlock_async(self: Rc<Self>) -> deno_runtime::deno_io::fs::FsResult<()> {
        Ok(())
    }

    fn truncate_sync(self: Rc<Self>, _len: u64) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    async fn truncate_async(self: Rc<Self>, _len: u64) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn utime_sync(
        self: Rc<Self>,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    async fn utime_async(
        self: Rc<Self>,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn as_stdio(self: Rc<Self>) -> deno_runtime::deno_io::fs::FsResult<std::process::Stdio> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn backing_fd(self: Rc<Self>) -> Option<deno_core::ResourceHandleFd> {
        None
    }

    fn try_clone_inner(
        self: Rc<Self>,
    ) -> deno_runtime::deno_io::fs::FsResult<Rc<dyn deno_runtime::deno_io::fs::File>> {
        let file = self
            .fs
            .new_open_options()
            .read(true)
            .open(&self.path)
            .map_err(|err| deno_runtime::deno_io::fs::FsError::from(io::Error::from(err)))?;
        Ok(Rc::new(BridgeFile {
            fs: self.fs.clone(),
            path: self.path.clone(),
            inner: tokio::sync::Mutex::new(file),
        }))
    }
}

impl<T> FsBridge<Arc<FsBridgeState<T>>>
where
    T: virtual_fs::FileSystem + Clone + Send + Sync + 'static,
{
    fn open_virtual(
        &self,
        path: &Path,
        options: deno_fs::OpenOptions,
    ) -> Result<
        Box<dyn virtual_fs::VirtualFile + Send + Sync + 'static>,
        deno_runtime::deno_io::fs::FsError,
    > {
        let mut opts = self.fs().new_open_options();
        opts.read(options.read)
            .write(options.write)
            .create(options.create)
            .truncate(options.truncate)
            .append(options.append)
            .create_new(options.create_new);
        opts.open(path)
            .map_err(|err| deno_runtime::deno_io::fs::FsError::from(io::Error::from(err)))
    }
}

#[async_trait::async_trait(?Send)]
impl<T> deno_fs::FileSystem for FsBridge<Arc<FsBridgeState<T>>>
where
    T: virtual_fs::FileSystem + Clone + Send + Sync + 'static,
{
    fn cwd(&self) -> deno_runtime::deno_io::fs::FsResult<PathBuf> {
        let cwd = self
            .cwd()
            .lock()
            .map_err(|_| deno_runtime::deno_io::fs::FsError::from(io::ErrorKind::Other))?;
        Ok(cwd.clone())
    }

    fn tmp_dir(&self) -> deno_runtime::deno_io::fs::FsResult<PathBuf> {
        Ok(PathBuf::from("/tmp"))
    }

    fn chdir(
        &self,
        path: &deno_permissions::CheckedPath,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        let path = path.as_ref();
        let meta = self
            .fs()
            .metadata(path)
            .map_err(|err| deno_runtime::deno_io::fs::FsError::from(io::Error::from(err)))?;
        if !meta.ft.is_dir() {
            return Err(deno_runtime::deno_io::fs::FsError::from(
                io::ErrorKind::NotADirectory,
            ));
        }
        let mut cwd = self
            .cwd()
            .lock()
            .map_err(|_| deno_runtime::deno_io::fs::FsError::from(io::ErrorKind::Other))?;
        *cwd = path.to_path_buf();
        Ok(())
    }

    fn umask(&self, _mask: Option<u32>) -> deno_runtime::deno_io::fs::FsResult<u32> {
        Ok(0)
    }

    fn open_sync(
        &self,
        path: &deno_permissions::CheckedPath,
        options: deno_fs::OpenOptions,
    ) -> deno_runtime::deno_io::fs::FsResult<Rc<dyn deno_runtime::deno_io::fs::File>> {
        let file = self.open_virtual(path, options)?;
        Ok(Rc::new(BridgeFile {
            fs: self.fs().clone(),
            path: path.to_path_buf(),
            inner: tokio::sync::Mutex::new(file),
        }))
    }

    async fn open_async<'a>(
        &'a self,
        path: deno_permissions::CheckedPathBuf,
        options: deno_fs::OpenOptions,
    ) -> deno_runtime::deno_io::fs::FsResult<Rc<dyn deno_runtime::deno_io::fs::File>> {
        let file = self.open_virtual(&path, options)?;
        Ok(Rc::new(BridgeFile {
            fs: self.fs().clone(),
            path: path.to_path_buf(),
            inner: tokio::sync::Mutex::new(file),
        }))
    }

    fn mkdir_sync(
        &self,
        _path: &deno_permissions::CheckedPath,
        _recursive: bool,
        _mode: Option<u32>,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    async fn mkdir_async(
        &self,
        _path: deno_permissions::CheckedPathBuf,
        _recursive: bool,
        _mode: Option<u32>,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    #[cfg(unix)]
    fn chmod_sync(
        &self,
        _path: &deno_permissions::CheckedPath,
        _mode: u32,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }
    #[cfg(not(unix))]
    fn chmod_sync(
        &self,
        _path: &deno_permissions::CheckedPath,
        _mode: i32,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    #[cfg(unix)]
    async fn chmod_async(
        &self,
        _path: deno_permissions::CheckedPathBuf,
        _mode: u32,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }
    #[cfg(not(unix))]
    async fn chmod_async(
        &self,
        _path: deno_permissions::CheckedPathBuf,
        _mode: i32,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn chown_sync(
        &self,
        _path: &deno_permissions::CheckedPath,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    async fn chown_async(
        &self,
        _path: deno_permissions::CheckedPathBuf,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn lchmod_sync(
        &self,
        _path: &deno_permissions::CheckedPath,
        _mode: u32,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    async fn lchmod_async(
        &self,
        _path: deno_permissions::CheckedPathBuf,
        _mode: u32,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn lchown_sync(
        &self,
        _path: &deno_permissions::CheckedPath,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    async fn lchown_async(
        &self,
        _path: deno_permissions::CheckedPathBuf,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn remove_sync(
        &self,
        _path: &deno_permissions::CheckedPath,
        _recursive: bool,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    async fn remove_async(
        &self,
        _path: deno_permissions::CheckedPathBuf,
        _recursive: bool,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn copy_file_sync(
        &self,
        _oldpath: &deno_permissions::CheckedPath,
        _newpath: &deno_permissions::CheckedPath,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    async fn copy_file_async(
        &self,
        _oldpath: deno_permissions::CheckedPathBuf,
        _newpath: deno_permissions::CheckedPathBuf,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn cp_sync(
        &self,
        _path: &deno_permissions::CheckedPath,
        _new_path: &deno_permissions::CheckedPath,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    async fn cp_async(
        &self,
        _path: deno_permissions::CheckedPathBuf,
        _new_path: deno_permissions::CheckedPathBuf,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn stat_sync(
        &self,
        path: &deno_permissions::CheckedPath,
    ) -> deno_runtime::deno_io::fs::FsResult<deno_runtime::deno_io::fs::FsStat> {
        let meta = self
            .fs()
            .metadata(path.as_ref())
            .map_err(|err| deno_runtime::deno_io::fs::FsError::from(io::Error::from(err)))?;
        Ok(stat_from_meta(meta))
    }

    async fn stat_async(
        &self,
        path: deno_permissions::CheckedPathBuf,
    ) -> deno_runtime::deno_io::fs::FsResult<deno_runtime::deno_io::fs::FsStat> {
        self.stat_sync(&path.as_checked_path())
    }

    fn lstat_sync(
        &self,
        path: &deno_permissions::CheckedPath,
    ) -> deno_runtime::deno_io::fs::FsResult<deno_runtime::deno_io::fs::FsStat> {
        let meta = self
            .fs()
            .symlink_metadata(path.as_ref())
            .map_err(|err| deno_runtime::deno_io::fs::FsError::from(io::Error::from(err)))?;
        Ok(stat_from_meta(meta))
    }

    async fn lstat_async(
        &self,
        path: deno_permissions::CheckedPathBuf,
    ) -> deno_runtime::deno_io::fs::FsResult<deno_runtime::deno_io::fs::FsStat> {
        self.lstat_sync(&path.as_checked_path())
    }

    fn realpath_sync(
        &self,
        path: &deno_permissions::CheckedPath,
    ) -> deno_runtime::deno_io::fs::FsResult<PathBuf> {
        Ok(virtual_fs::host_fs::normalize_path(path.as_ref()))
    }

    async fn realpath_async(
        &self,
        path: deno_permissions::CheckedPathBuf,
    ) -> deno_runtime::deno_io::fs::FsResult<PathBuf> {
        self.realpath_sync(&path.as_checked_path())
    }

    fn read_dir_sync(
        &self,
        path: &deno_permissions::CheckedPath,
    ) -> deno_runtime::deno_io::fs::FsResult<Vec<deno_fs::FsDirEntry>> {
        let entries = self
            .fs()
            .read_dir(path.as_ref())
            .map_err(|err| deno_runtime::deno_io::fs::FsError::from(io::Error::from(err)))?;
        let mapped = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| deno_fs::FsDirEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                is_file: entry.file_type().map(|t| t.is_file()).unwrap_or(false),
                is_directory: entry.file_type().map(|t| t.is_dir()).unwrap_or(false),
                is_symlink: entry.file_type().map(|t| t.is_symlink()).unwrap_or(false),
            })
            .collect();
        Ok(mapped)
    }

    async fn read_dir_async(
        &self,
        path: deno_permissions::CheckedPathBuf,
    ) -> deno_runtime::deno_io::fs::FsResult<Vec<deno_fs::FsDirEntry>> {
        self.read_dir_sync(&path.as_checked_path())
    }

    fn rename_sync(
        &self,
        _oldpath: &deno_permissions::CheckedPath,
        _newpath: &deno_permissions::CheckedPath,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    async fn rename_async(
        &self,
        _oldpath: deno_permissions::CheckedPathBuf,
        _newpath: deno_permissions::CheckedPathBuf,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn link_sync(
        &self,
        _oldpath: &deno_permissions::CheckedPath,
        _newpath: &deno_permissions::CheckedPath,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    async fn link_async(
        &self,
        _oldpath: deno_permissions::CheckedPathBuf,
        _newpath: deno_permissions::CheckedPathBuf,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn symlink_sync(
        &self,
        _oldpath: &deno_permissions::CheckedPath,
        _newpath: &deno_permissions::CheckedPath,
        _file_type: Option<deno_fs::FsFileType>,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    async fn symlink_async(
        &self,
        _oldpath: deno_permissions::CheckedPathBuf,
        _newpath: deno_permissions::CheckedPathBuf,
        _file_type: Option<deno_fs::FsFileType>,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn read_link_sync(
        &self,
        path: &deno_permissions::CheckedPath,
    ) -> deno_runtime::deno_io::fs::FsResult<PathBuf> {
        self.fs()
            .readlink(path.as_ref())
            .map_err(|err| deno_runtime::deno_io::fs::FsError::from(io::Error::from(err)))
    }

    async fn read_link_async(
        &self,
        path: deno_permissions::CheckedPathBuf,
    ) -> deno_runtime::deno_io::fs::FsResult<PathBuf> {
        self.read_link_sync(&path.as_checked_path())
    }

    fn truncate_sync(
        &self,
        _path: &deno_permissions::CheckedPath,
        _len: u64,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    async fn truncate_async(
        &self,
        _path: deno_permissions::CheckedPathBuf,
        _len: u64,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn utime_sync(
        &self,
        _path: &deno_permissions::CheckedPath,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    async fn utime_async(
        &self,
        _path: deno_permissions::CheckedPathBuf,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn lutime_sync(
        &self,
        _path: &deno_permissions::CheckedPath,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    async fn lutime_async(
        &self,
        _path: deno_permissions::CheckedPathBuf,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> deno_runtime::deno_io::fs::FsResult<()> {
        Err(deno_runtime::deno_io::fs::FsError::NotSupported)
    }

    fn exists_sync(&self, path: &deno_permissions::CheckedPath) -> bool {
        self.fs().metadata(path.as_ref()).is_ok()
    }

    async fn exists_async(
        &self,
        path: deno_permissions::CheckedPathBuf,
    ) -> deno_runtime::deno_io::fs::FsResult<bool> {
        Ok(self.exists_sync(&path.as_checked_path()))
    }
}

pub(crate) struct FsNodeRequireLoader<T> {
    pub(crate) bridge: FsBridge<Arc<FsBridgeState<T>>>,
    pub(crate) pkg_json_resolver: PackageJsonResolverRc<FsBridge<Arc<FsBridgeState<T>>>>,
}

impl<T> deno_runtime::deno_node::NodeRequireLoader for FsNodeRequireLoader<T>
where
    T: virtual_fs::FileSystem + Clone + Send + Sync + 'static,
{
    fn ensure_read_permission<'a>(
        &self,
        _permissions: &mut deno_runtime::deno_permissions::PermissionsContainer,
        path: Cow<'a, Path>,
    ) -> Result<Cow<'a, Path>, JsErrorBox> {
        Ok(path)
    }

    fn load_text_file_lossy(&self, path: &Path) -> Result<deno_core::FastString, JsErrorBox> {
        let mut file = self
            .bridge
            .fs()
            .new_open_options()
            .read(true)
            .open(path)
            .map_err(|err| JsErrorBox::from_err(io::Error::from(err)))?;
        let mut buffer = String::new();
        tokio::runtime::Handle::current().block_on(async {
            file.read_to_string(&mut buffer)
                .await
                .map_err(JsErrorBox::from_err)
        })?;
        Ok(deno_core::FastString::from(buffer))
    }

    fn is_maybe_cjs(&self, specifier: &deno_core::url::Url) -> Result<bool, PackageJsonLoadError> {
        let path = specifier.to_file_path().map_err(|_| {
            PackageJsonLoadError(deno_package_json::PackageJsonLoadError::Io {
                path: PathBuf::from("<invalid>"),
                source: std::io::Error::new(std::io::ErrorKind::Other, "invalid file path"),
            })
        })?;
        let pkg_json = self.pkg_json_resolver.get_closest_package_json(&path)?;
        Ok(pkg_json.map(|p| p.typ != "module").unwrap_or(true))
    }
}
