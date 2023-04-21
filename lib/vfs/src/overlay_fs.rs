use std::{
    fmt::Debug,
    path::{Path, PathBuf},
    pin::Pin,
    task::Poll,
};

use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};

use crate::{
    ops, FileOpener, FileSystem, FileSystems, FsError, Metadata, OpenOptions, OpenOptionsConfig,
    ReadDir, VirtualFile,
};

/// A primary filesystem and chain of secondary filesystems that are overlayed
/// on top of each other.
///
/// # Precedence
///
/// The [`OverlayFileSystem`] will execute operations based on precedence.
///
///
/// Most importantly, this means earlier filesystems can shadow files and
/// directories that have a lower precedence.
///
///# Examples
///
/// Something useful to know is that the [`FileSystems`] trait is implemented
/// for both arrays and tuples.
///
/// For example, if you want to create a [`crate::FileSystem`] which will
/// create files in-memory while still being able to read from the host, you
/// might do something like this:
///
/// ```rust
/// use virtual_fs::{
///     mem_fs::FileSystem as MemFS,
///     host_fs::FileSystem as HostFS,
///     OverlayFileSystem,
/// };
/// let fs = OverlayFileSystem::new(MemFS::default(), [HostFS]);
///
/// // This also has the benefit of storing the two values in-line with no extra
/// // overhead or indirection.
/// assert_eq!(
///     std::mem::size_of_val(&fs),
///     std::mem::size_of::<(MemFS, HostFS)>(),
/// );
/// ```
///
/// A more complex example is
#[derive(Clone, PartialEq, Eq)]
pub struct OverlayFileSystem<P, S> {
    primary: P,
    secondaries: S,
}

impl<P, S> OverlayFileSystem<P, S>
where
    P: FileSystem + 'static,
    S: for<'a> FileSystems<'a> + Send + Sync + 'static,
{
    /// Create a new [`FileSystem`] using a primary [`crate::FileSystem`] and a
    /// chain of secondary [`FileSystems`].
    pub fn new(primary: P, secondaries: S) -> Self {
        OverlayFileSystem {
            primary,
            secondaries,
        }
    }

    /// Get a reference to the primary filesystem.
    pub fn primary(&self) -> &P {
        &self.primary
    }

    /// Get a mutable reference to the primary filesystem.
    pub fn primary_mut(&mut self) -> &mut P {
        &mut self.primary
    }

    /// Get a reference to the secondary filesystems.
    pub fn secondaries(&self) -> &S {
        &self.secondaries
    }

    /// Get a mutable reference to the secondary filesystems.
    pub fn secondaries_mut(&mut self) -> &mut S {
        &mut self.secondaries
    }

    /// Consume the [`OverlayFileSystem`], returning the underlying primary and
    /// secondary filesystems.
    pub fn into_inner(self) -> (P, S) {
        (self.primary, self.secondaries)
    }

    fn permission_error_or_not_found(&self, path: &Path) -> Result<(), FsError> {
        for fs in self.secondaries.filesystems() {
            if ops::exists(fs, path) {
                return Err(FsError::PermissionDenied);
            }
        }

        Err(FsError::EntryNotFound)
    }
}

impl<P, S> FileSystem for OverlayFileSystem<P, S>
where
    P: FileSystem + 'static,
    S: for<'a> FileSystems<'a> + Send + Sync + 'static,
{
    fn read_dir(&self, path: &Path) -> Result<ReadDir, FsError> {
        let mut entries = Vec::new();
        let mut had_at_least_one_success = false;

        let filesystems = std::iter::once(&self.primary as &dyn FileSystem)
            .into_iter()
            .chain(self.secondaries().filesystems());

        for fs in filesystems {
            match fs.read_dir(path) {
                Ok(r) => {
                    for entry in r {
                        entries.push(entry?);
                    }
                    had_at_least_one_success = true;
                }
                Err(e) if should_continue(e) => continue,
                Err(e) => return Err(e),
            }
        }

        if had_at_least_one_success {
            // Make sure later entries are removed in favour of earlier ones.
            // Note: this sort is guaranteed to be stable, meaning filesystems
            // "higher up" the chain will be further towards the start and kept
            // when deduplicating.
            entries.sort_by(|a, b| a.path.cmp(&b.path));
            entries.dedup_by(|a, b| a.path == b.path);

            Ok(ReadDir::new(entries))
        } else {
            Err(FsError::BaseNotDirectory)
        }
    }

    fn create_dir(&self, path: &Path) -> Result<(), FsError> {
        match self.primary.create_dir(path) {
            Err(e) if should_continue(e) => {}
            other => return other,
        }

        self.permission_error_or_not_found(path)
    }

    fn remove_dir(&self, path: &Path) -> Result<(), FsError> {
        match self.primary.remove_dir(path) {
            Err(e) if should_continue(e) => {}
            other => return other,
        }

        self.permission_error_or_not_found(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        match self.primary.rename(from, to) {
            Err(e) if should_continue(e) => {}
            other => return other,
        }

        self.permission_error_or_not_found(from)
    }

    fn metadata(&self, path: &Path) -> Result<Metadata, FsError> {
        match self.primary.metadata(path) {
            Ok(meta) => return Ok(meta),
            Err(e) if should_continue(e) => {}
            Err(e) => return Err(e),
        }

        for fs in self.secondaries.filesystems() {
            match fs.metadata(path) {
                Err(e) if should_continue(e) => continue,
                other => return other,
            }
        }

        Err(FsError::EntryNotFound)
    }

    fn remove_file(&self, path: &Path) -> Result<(), FsError> {
        match self.primary.remove_file(path) {
            Err(e) if should_continue(e) => {}
            other => return other,
        }

        self.permission_error_or_not_found(path)
    }

    fn new_open_options(&self) -> OpenOptions<'_> {
        OpenOptions::new(self)
    }
}

impl<P, S> FileOpener for OverlayFileSystem<P, S>
where
    P: FileSystem,
    S: for<'a> FileSystems<'a> + Send + Sync + 'static,
{
    fn open(
        &self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>, FsError> {
        match self
            .primary
            .new_open_options()
            .options(conf.clone())
            .open(path)
        {
            Err(e) if should_continue(e) => {}
            other => return other,
        }

        if (conf.create || conf.create_new) && !ops::exists(self, path) {
            if let Some(parent) = path.parent() {
                let parent_exists_on_secondary_fs = self
                    .secondaries
                    .filesystems()
                    .into_iter()
                    .any(|fs| ops::is_dir(fs, parent));
                if parent_exists_on_secondary_fs {
                    // We fall into the special case where you can create a file
                    // that looks like it is inside a secondary filesystem folder,
                    // but actually it gets created on the host
                    ops::create_dir_all(&self.primary, parent)?;
                    return self
                        .primary
                        .new_open_options()
                        .options(conf.clone())
                        .open(path);
                } else {
                    return Err(FsError::EntryNotFound);
                }
            }
        }

        if opening_would_require_mutations(&self.secondaries, path, conf) {
            // HACK: we should return Err(FsError::PermissionDenied) here
            return open_readonly_file_hack(path, conf, &self.secondaries);
        }

        for fs in self.secondaries.filesystems() {
            match fs.new_open_options().options(conf.clone()).open(path) {
                Err(e) if should_continue(e) => continue,
                other => return other,
            }
        }

        Err(FsError::EntryNotFound)
    }
}

/// HACK(Michael-F-Bryan): In theory, you shouldn't be able to open a file in
/// one of the [`OverlayFileSystem`]'s secondaries in write mode because the
/// filesystem is meant to be readonly. However, Python does things like
/// `open("./lib/python3.6/io.py", "rw")` when importing its standard library
/// and we want Python to work, so we'll defer the [`FsError::PermissionDenied`]
/// error until the first write operation.
///
/// We shouldn't need to do this because opening a secondary fs's file in write
/// mode goes against the "read-write primary, readonly secondaries" goal.
fn open_readonly_file_hack<S>(
    path: &Path,
    conf: &OpenOptionsConfig,
    secondaries: &S,
) -> Result<Box<dyn VirtualFile + Send + Sync>, FsError>
where
    S: for<'a> FileSystems<'a> + Send + Sync + 'static,
{
    #[derive(Debug)]
    struct ReadOnlyFile {
        path: PathBuf,
        inner: Box<dyn VirtualFile + Send + Sync>,
    }

    impl VirtualFile for ReadOnlyFile {
        fn last_accessed(&self) -> u64 {
            self.inner.last_accessed()
        }

        fn last_modified(&self) -> u64 {
            self.inner.last_modified()
        }

        fn created_time(&self) -> u64 {
            self.inner.created_time()
        }

        fn size(&self) -> u64 {
            self.inner.size()
        }

        fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
            self.inner.set_len(new_size)
        }

        fn unlink(&mut self) -> crate::Result<()> {
            Err(FsError::PermissionDenied)
        }

        fn poll_read_ready(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<std::io::Result<usize>> {
            Pin::new(&mut *self.inner).poll_read_ready(cx)
        }

        fn poll_write_ready(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<std::io::Result<usize>> {
            Pin::new(&mut *self.inner).poll_write_ready(cx)
        }
    }

    impl AsyncWrite for ReadOnlyFile {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            _buf: &[u8],
        ) -> Poll<Result<usize, std::io::Error>> {
            tracing::warn!(
                path=%self.path.display(),
                "Attempting to write to a readonly file",
            );
            Poll::Ready(Err(std::io::ErrorKind::PermissionDenied.into()))
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            tracing::warn!(
                path=%self.path.display(),
                "Attempting to flush a readonly file",
            );
            Poll::Ready(Err(std::io::ErrorKind::PermissionDenied.into()))
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            tracing::warn!(
                path=%self.path.display(),
                "Attempting to shutdown a readonly file",
            );
            Poll::Ready(Err(std::io::ErrorKind::PermissionDenied.into()))
        }
    }

    impl AsyncRead for ReadOnlyFile {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut *self.inner).poll_read(cx, buf)
        }
    }

    impl AsyncSeek for ReadOnlyFile {
        fn start_seek(
            mut self: Pin<&mut Self>,
            position: std::io::SeekFrom,
        ) -> std::io::Result<()> {
            Pin::new(&mut *self.inner).start_seek(position)
        }

        fn poll_complete(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<std::io::Result<u64>> {
            Pin::new(&mut *self.inner).poll_complete(cx)
        }
    }

    for fs in secondaries.filesystems() {
        match fs.new_open_options().options(conf.clone()).open(path) {
            Ok(f) => {
                return Ok(Box::new(ReadOnlyFile {
                    path: path.to_path_buf(),
                    inner: f,
                }));
            }
            Err(e) if should_continue(e) => continue,
            other => return other,
        }
    }

    Err(FsError::EntryNotFound)
}

fn opening_would_require_mutations<S>(
    secondaries: &S,
    path: &Path,
    conf: &OpenOptionsConfig,
) -> bool
where
    S: for<'a> FileSystems<'a> + Send + Sync,
{
    if conf.append || conf.write || conf.create_new | conf.truncate {
        return true;
    }

    if conf.create {
        // Would we create the file if it doesn't exist yet?
        let already_exists = secondaries
            .filesystems()
            .into_iter()
            .any(|fs| ops::is_file(fs, path));

        if !already_exists {
            return true;
        }
    }

    false
}

impl<P, S> Debug for OverlayFileSystem<P, S>
where
    P: FileSystem,
    S: for<'a> FileSystems<'a>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        struct IterFilesystems<'a, S>(&'a S);
        impl<'a, S> Debug for IterFilesystems<'a, S>
        where
            S: for<'b> FileSystems<'b>,
        {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let mut f = f.debug_list();

                for fs in self.0.filesystems() {
                    f.entry(&fs);
                }

                f.finish()
            }
        }

        f.debug_struct("OverlayFileSystem")
            .field("primary", &self.primary)
            .field("secondaries", &IterFilesystems(&self.secondaries))
            .finish()
    }
}

fn should_continue(e: FsError) -> bool {
    // HACK: We shouldn't really be ignoring FsError::BaseNotDirectory, but
    // it's needed because the mem_fs::FileSystem doesn't return
    // FsError::EntryNotFound when an intermediate directory doesn't exist
    // (i.e. the "/path/to" in "/path/to/file.txt").
    matches!(
        e,
        FsError::EntryNotFound | FsError::InvalidInput | FsError::BaseNotDirectory
    )
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use bytes::Bytes;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use webc::v1::{ParseOptions, WebCOwned};

    use super::*;
    use crate::{mem_fs::FileSystem as MemFS, webc_fs::WebcFileSystem, RootFileSystemBuilder};

    const PYTHON: &[u8] = include_bytes!("../../c-api/examples/assets/python-0.1.0.wasmer");

    #[test]
    fn object_safe() {
        fn _box_with_memfs(
            fs: OverlayFileSystem<MemFS, Vec<MemFS>>,
        ) -> Box<dyn crate::FileSystem + Send + Sync + 'static> {
            Box::new(fs)
        }

        fn _arc<A, S>(fs: OverlayFileSystem<A, S>) -> Arc<dyn crate::FileSystem + 'static>
        where
            A: FileSystem + 'static,
            S: for<'a> FileSystems<'a> + Send + Sync + Debug + 'static,
        {
            Arc::new(fs)
        }
    }

    #[tokio::test]
    async fn remove_directory() {
        let primary = MemFS::default();
        let secondary = MemFS::default();
        let first = Path::new("/first");
        let second = Path::new("/second");
        let file_txt = second.join("file.txt");
        let third = Path::new("/third");
        primary.create_dir(first).unwrap();
        primary.create_dir(second).unwrap();
        primary
            .new_open_options()
            .create(true)
            .write(true)
            .open(&file_txt)
            .unwrap()
            .write_all(b"Hello, World!")
            .await
            .unwrap();
        secondary.create_dir(third).unwrap();

        let overlay = OverlayFileSystem::new(primary, [secondary]);

        // Delete a folder on the primary filesystem
        overlay.remove_dir(first).unwrap();
        assert_eq!(
            overlay.primary().metadata(first).unwrap_err(),
            FsError::EntryNotFound,
            "Deleted from primary"
        );
        assert!(!ops::exists(&overlay.secondaries[0], second));

        // Directory on the primary fs isn't empty
        assert_eq!(
            overlay.remove_dir(second).unwrap_err(),
            FsError::DirectoryNotEmpty,
        );

        // Try to remove something on one of the overlay filesystems
        assert_eq!(
            overlay.remove_dir(third).unwrap_err(),
            FsError::PermissionDenied,
        );
        assert!(ops::exists(&overlay.secondaries[0], third));
    }

    #[tokio::test]
    async fn open_files() {
        let primary = MemFS::default();
        let secondary = MemFS::default();
        ops::create_dir_all(&primary, "/primary").unwrap();
        ops::touch(&primary, "/primary/read.txt").unwrap();
        ops::touch(&primary, "/primary/write.txt").unwrap();
        ops::create_dir_all(&secondary, "/secondary").unwrap();
        ops::touch(&secondary, "/secondary/read.txt").unwrap();
        ops::touch(&secondary, "/secondary/write.txt").unwrap();
        ops::create_dir_all(&secondary, "/primary").unwrap();
        ops::write(&secondary, "/primary/read.txt", "This is shadowed")
            .await
            .unwrap();

        let fs = OverlayFileSystem::new(primary, [secondary]);

        // Any new files will be created on the primary fs
        let _ = fs
            .new_open_options()
            .create(true)
            .write(true)
            .open("/new.txt")
            .unwrap();
        assert!(ops::exists(&fs.primary, "/new.txt"));
        assert!(!ops::exists(&fs.secondaries[0], "/new.txt"));

        // You can open a file for reading and writing on the primary fs
        let _ = fs
            .new_open_options()
            .create(false)
            .write(true)
            .read(true)
            .open("/primary/write.txt")
            .unwrap();

        // Files on the primary should always shadow the secondary
        let content = ops::read_to_string(&fs, "/primary/read.txt").await.unwrap();
        assert_ne!(content, "This is shadowed");
    }

    #[test]
    fn create_file_that_looks_like_it_is_in_a_secondary_filesystem_folder() {
        let primary = MemFS::default();
        let secondary = MemFS::default();
        ops::create_dir_all(&secondary, "/path/to/").unwrap();
        assert!(!ops::is_dir(&primary, "/path/to/"));
        let fs = OverlayFileSystem::new(primary, [secondary]);

        ops::touch(&fs, "/path/to/file.txt").unwrap();

        assert!(ops::is_dir(&fs.primary, "/path/to/"));
        assert!(ops::is_file(&fs.primary, "/path/to/file.txt"));
        assert!(!ops::is_file(&fs.secondaries[0], "/path/to/file.txt"));
    }

    #[tokio::test]
    async fn listed_files_appear_overlayed() {
        let primary = MemFS::default();
        let secondary = MemFS::default();
        let secondary_overlayed = MemFS::default();
        ops::create_dir_all(&primary, "/primary").unwrap();
        ops::touch(&primary, "/primary/read.txt").unwrap();
        ops::touch(&primary, "/primary/write.txt").unwrap();
        ops::create_dir_all(&secondary, "/secondary").unwrap();
        ops::touch(&secondary, "/secondary/read.txt").unwrap();
        ops::touch(&secondary, "/secondary/write.txt").unwrap();
        // This second "secondary" filesystem should share the same folders as
        // the first one.
        ops::create_dir_all(&secondary_overlayed, "/secondary").unwrap();
        ops::touch(&secondary_overlayed, "/secondary/overlayed.txt").unwrap();

        let fs = OverlayFileSystem::new(primary, [secondary, secondary_overlayed]);

        let paths: Vec<_> = ops::walk(&fs, "/").map(|entry| entry.path()).collect();
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/secondary"),
                PathBuf::from("/secondary/write.txt"),
                PathBuf::from("/secondary/read.txt"),
                PathBuf::from("/secondary/overlayed.txt"),
                PathBuf::from("/primary"),
                PathBuf::from("/primary/write.txt"),
                PathBuf::from("/primary/read.txt"),
            ]
        );
    }

    #[tokio::test]
    async fn wasi_runner_use_case() {
        // Set up some dummy files on the host
        let temp = TempDir::new().unwrap();
        let first = temp.path().join("first");
        let file_txt = first.join("file.txt");
        let second = temp.path().join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::write(&file_txt, b"First!").unwrap();
        std::fs::create_dir_all(&second).unwrap();
        // configure the union FS so things are saved in memory by default
        // (initialized with a set of unix-like folders), but certain folders
        // are first to the host.
        let primary = RootFileSystemBuilder::new().build();
        let host_fs: Arc<dyn FileSystem + Send + Sync> = Arc::new(crate::host_fs::FileSystem);
        let first_dirs = [(&first, "/first"), (&second, "/second")];
        for (host, guest) in first_dirs {
            primary
                .mount(PathBuf::from(guest), &host_fs, host.clone())
                .unwrap();
        }
        // Set up the secondary file systems
        let webc = WebCOwned::parse(Bytes::from_static(PYTHON), &ParseOptions::default()).unwrap();
        let webc = WebcFileSystem::init_all(Arc::new(webc));

        let fs = OverlayFileSystem::new(primary, [webc]);

        // We should get all the normal directories from rootfs (primary)
        assert!(ops::is_dir(&fs, "/lib"));
        assert!(ops::is_dir(&fs, "/bin"));
        assert!(ops::is_file(&fs, "/dev/stdin"));
        assert!(ops::is_file(&fs, "/dev/stdout"));
        // We also want to see files from the WEBC volumes (secondary)
        assert!(ops::is_dir(&fs, "/lib/python3.6"));
        assert!(ops::is_file(&fs, "/lib/python3.6/collections/__init__.py"));
        #[cfg(never)]
        {
            // files on a secondary fs aren't writable
            // TODO(Michael-F-Bryan): re-enable this if/when we fix
            // open_readonly_file_hack()
            assert_eq!(
                fs.new_open_options()
                    .append(true)
                    .open("/lib/python3.6/collections/__init__.py")
                    .unwrap_err(),
                FsError::PermissionDenied,
            );
        }
        // you are allowed to create files that look like they are in a secondary
        // folder, though
        ops::touch(&fs, "/lib/python3.6/collections/something-else.py").unwrap();
        // But it'll be on the primary filesystem, not the secondary one
        assert!(ops::is_file(
            &fs.primary,
            "/lib/python3.6/collections/something-else.py"
        ));
        assert!(!ops::is_file(
            &fs.secondaries[0],
            "/lib/python3.6/collections/something-else.py"
        ));
        // You can do the same thing with folders
        fs.create_dir("/lib/python3.6/something-else".as_ref())
            .unwrap();
        assert!(ops::is_dir(&fs.primary, "/lib/python3.6/something-else"));
        assert!(!ops::is_dir(
            &fs.secondaries[0],
            "/lib/python3.6/something-else"
        ));
        // It only works when you are directly inside an existing directory
        // on the secondary filesystem, though
        assert_eq!(
            ops::touch(&fs, "/lib/python3.6/collections/this/doesnt/exist.txt").unwrap_err(),
            FsError::EntryNotFound
        );
        // you should also be able to read files mounted from the host
        assert!(ops::is_dir(&fs, "/first"));
        assert!(ops::is_file(&fs, "/first/file.txt"));
        assert_eq!(
            ops::read_to_string(&fs, "/first/file.txt").await.unwrap(),
            "First!"
        );
        // Overwriting them is fine and we'll see the changes on the host
        ops::write(&fs, "/first/file.txt", "Updated").await.unwrap();
        assert_eq!(std::fs::read_to_string(&file_txt).unwrap(), "Updated");
        // The filesystem will see changes on the host that happened after it was
        // set up
        let another = second.join("another.txt");
        std::fs::write(&another, "asdf").unwrap();
        assert_eq!(
            ops::read_to_string(&fs, "/second/another.txt")
                .await
                .unwrap(),
            "asdf"
        );
    }

    fn load_webc(bytes: &'static [u8]) -> WebcFileSystem<WebCOwned> {
        let options = ParseOptions::default();
        let webc = WebCOwned::parse(bytes, &options).unwrap();
        WebcFileSystem::init_all(Arc::new(webc))
    }

    #[track_caller]
    fn assert_same_directory_contents(
        original: &dyn FileSystem,
        path: impl AsRef<Path>,
        candidate: &dyn FileSystem,
    ) {
        let path = path.as_ref();

        let original_entries: Vec<_> = original
            .read_dir(path)
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        let candidate_entries: Vec<_> = candidate
            .read_dir(path)
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(original_entries, candidate_entries);
    }

    #[test]
    fn absolute_and_relative_paths_are_passed_through() {
        let python = Arc::new(load_webc(PYTHON));

        // The underlying filesystem doesn't care about absolute/relative paths
        assert_eq!(python.read_dir("/lib".as_ref()).unwrap().count(), 4);
        assert_eq!(python.read_dir("lib".as_ref()).unwrap().count(), 4);

        // read_dir() should be passed through to the primary
        let webc_primary =
            OverlayFileSystem::new(Arc::clone(&python), [crate::EmptyFileSystem::default()]);
        assert_same_directory_contents(&python, "/lib", &webc_primary);
        assert_same_directory_contents(&python, "lib", &webc_primary);

        // read_dir() should also be passed through to the secondary
        let webc_secondary =
            OverlayFileSystem::new(crate::EmptyFileSystem::default(), [Arc::clone(&python)]);
        assert_same_directory_contents(&python, "/lib", &webc_secondary);
        assert_same_directory_contents(&python, "lib", &webc_secondary);

        // It should be fine to overlay the root fs on top of our webc file
        let overlay_rootfs = OverlayFileSystem::new(
            RootFileSystemBuilder::default().build(),
            [Arc::clone(&python)],
        );
        assert_same_directory_contents(&python, "/lib", &overlay_rootfs);
        assert_same_directory_contents(&python, "lib", &overlay_rootfs);
    }

    #[tokio::test]
    async fn open_secondary_fs_files_in_write_mode_and_error_on_first_write() {
        // TODO(Michael-F-Bryan): remove this test if/when we fix
        // open_readonly_file_hack()
        let primary = MemFS::default();
        let secondary = MemFS::default();
        ops::create_dir_all(&secondary, "/secondary").unwrap();
        ops::write(&secondary, "/secondary/file.txt", b"Hello, World!")
            .await
            .unwrap();

        let fs = OverlayFileSystem::new(primary, [secondary]);

        let mut f = fs
            .new_open_options()
            .write(true)
            .read(true)
            .open("/secondary/file.txt")
            .unwrap();
        // reading is fine
        let mut buf = String::new();
        f.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, "Hello, World!");
        // but trying to write will error out
        assert_eq!(
            f.write(b"..").await.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied,
        );
        // Same with flushing and shutdown
        assert_eq!(
            f.flush().await.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied,
        );
        assert_eq!(
            f.shutdown().await.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied,
        );
    }
}
