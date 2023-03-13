use std::{fmt::Debug, path::Path};

use crate::{
    FileOpener, FileSystems, FsError, Metadata, OpenOptions, OpenOptionsConfig, ReadDir,
    VirtualFile,
};

/// A primary read-write filesystem and chain of read-only secondary filesystems
/// that are overlayed on top of each other.
///
/// # Precedence
///
/// The general rule is that mutating operations (e.g.
/// [`crate::FileSystem::remove_dir()`] or [`FileOpener::open()`] with
/// [`OpenOptions::write()`] set) will only be executed against the "primary"
/// filesystem.
///
/// For operations which don't make modifications (e.g. [`FileOpener::open()`]
/// in read-only mode), [`FileSystem`] will first check the primary filesystem,
/// and if that fails it will iterate through the secondary filesystems until
/// either one of them succeeds or there are no more filesystems.
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
/// use wasmer_vfs::{
///     mem_fs::FileSystem as MemFS,
///     host_fs::FileSystem as HostFS,
///     overlay_fs::FileSystem as OverlayFS,
/// };
/// let fs = OverlayFS::new(MemFS::default(), [HostFS]);
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
pub struct FileSystem<P, S>
where
    P: crate::FileSystem,
    S: for<'a> FileSystems<'a>,
{
    primary: P,
    secondaries: S,
}

impl<P, S> FileSystem<P, S>
where
    P: crate::FileSystem,
    S: for<'a> FileSystems<'a>,
{
    /// Create a new [`FileSystem`] using a primary [`crate::FileSystem`] and
    /// a chain of read-only secondary [`FileSystems`].
    pub fn new(primary: P, secondaries: S) -> Self {
        FileSystem {
            primary,
            secondaries,
        }
    }

    pub fn primary(&self) -> &P {
        &self.primary
    }

    pub fn primary_mut(&mut self) -> &mut P {
        &mut self.primary
    }

    pub fn secondaries(&self) -> &S {
        &self.secondaries
    }

    pub fn secondaries_mut(&mut self) -> &mut S {
        &mut self.secondaries
    }

    pub fn into_inner(self) -> (P, S) {
        (self.primary, self.secondaries)
    }

    /// Iterate over all filesystems in order of precedence.
    pub fn iter(&self) -> impl Iterator<Item = &'_ dyn crate::FileSystem> + '_ {
        std::iter::once(self.primary() as &dyn crate::FileSystem)
            .chain(self.secondaries().iter_filesystems())
    }
}

impl<P, S> crate::FileSystem for FileSystem<P, S>
where
    P: crate::FileSystem,
    S: for<'a> crate::FileSystems<'a> + Send + Sync,
{
    fn read_dir(&self, path: &Path) -> Result<ReadDir, FsError> {
        let mut entries = Vec::new();
        let mut had_at_least_one_success = false;

        for fs in self.iter() {
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
            // Note: this sort is guaranteed to be stable, so it will prefer
            // filesystems "higher up" the chain.
            entries.sort_by(|a, b| a.path.cmp(&b.path));
            // Make sure earlier entries shadow layer ones.
            entries.dedup_by(|a, b| a.path == b.path);

            Ok(ReadDir::new(entries))
        } else {
            Err(FsError::BaseNotDirectory)
        }
    }

    fn create_dir(&self, _path: &Path) -> Result<(), FsError> {
        todo!()
    }

    fn remove_dir(&self, path: &Path) -> Result<(), FsError> {
        match self.primary.remove_dir(path) {
            Ok(_) => return Ok(()),
            Err(e) if should_continue(e) => {
                // It's not in the primary filesystem, so we'll check the
                // secondaries to see whether we need to return a permission
                // error or a not found error.
            }
            Err(other) => return Err(other),
        }

        for fs in self.secondaries().iter_filesystems() {
            match fs.metadata(path) {
                Ok(m) if m.is_dir() => {
                    return Err(FsError::PermissionDenied);
                }
                Ok(_) => return Err(FsError::BaseNotDirectory),
                Err(e) if should_continue(e) => continue,
                Err(e) => return Err(e),
            }
        }

        Err(FsError::BaseNotDirectory)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        match self.primary.rename(from, to) {
            Ok(_) => return Ok(()),
            Err(e) if should_continue(e) => {}
            Err(e) => return Err(e),
        }

        for fs in self.secondaries().iter_filesystems() {
            if fs.metadata(from).is_ok() {
                return Err(FsError::PermissionDenied);
            }
        }

        Err(FsError::EntryNotFound)
    }

    fn metadata(&self, path: &Path) -> Result<Metadata, FsError> {
        for fs in self.iter() {
            match fs.metadata(path) {
                Ok(meta) => return Ok(meta),
                Err(e) if should_continue(e) => continue,
                Err(e) => return Err(e),
            }
        }

        Err(FsError::EntryNotFound)
    }

    fn remove_file(&self, path: &Path) -> Result<(), FsError> {
        match self.primary.remove_file(path) {
            Ok(_) => return Ok(()),
            Err(e) if should_continue(e) => {}
            Err(e) => return Err(e),
        }

        for fs in self.secondaries.iter_filesystems() {
            match fs.metadata(path) {
                Ok(meta) if meta.is_file() => {
                    return Err(FsError::PermissionDenied);
                }
                Ok(_) => return Err(FsError::NotAFile),
                Err(FsError::EntryNotFound) => {}
                Err(e) => return Err(e),
            }
        }

        Err(FsError::EntryNotFound)
    }

    fn new_open_options(&self) -> OpenOptions<'_> {
        OpenOptions::new(self)
    }
}

impl<P, S> FileOpener for FileSystem<P, S>
where
    P: crate::FileSystem,
    S: for<'a> FileSystems<'a>,
{
    fn open(
        &self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>, FsError> {
        // First, try the primary filesystem
        match self
            .primary
            .new_open_options()
            .options(conf.clone())
            .open(path)
        {
            Ok(f) => return Ok(f),
            Err(e) if should_continue(e) => {}
            Err(e) => return Err(e),
        };

        if conf.would_mutate() {
            return Err(FsError::PermissionDenied);
        }

        for fs in self.secondaries.iter_filesystems() {
            match fs.new_open_options().options(conf.clone()).open(path) {
                Ok(f) => return Ok(f),
                Err(e) if should_continue(e) => continue,
                Err(e) => return Err(e),
            }
        }

        Err(FsError::EntryNotFound)
    }
}

impl<P, S> Debug for FileSystem<P, S>
where
    P: crate::FileSystem,
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

                for fs in self.0.iter_filesystems() {
                    f.entry(&fs);
                }

                f.finish()
            }
        }

        f.debug_struct("FileSystem")
            .field("primary", &self.primary)
            .field("secondaries", &IterFilesystems(&self.secondaries))
            .finish()
    }
}

/// Is it okay to use a fallback filesystem to deal with this particular error?
fn should_continue(e: FsError) -> bool {
    matches!(e, FsError::EntryNotFound)
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use tokio::io::AsyncWriteExt;

    use super::*;
    use crate::{mem_fs::FileSystem as MemFS, FileSystem as _, FileSystemExt};

    #[test]
    fn can_be_used_as_an_object() {
        fn _box_with_memfs(
            fs: FileSystem<MemFS, Vec<MemFS>>,
        ) -> Box<dyn crate::FileSystem + Send + Sync + 'static> {
            Box::new(fs)
        }

        fn _arc(
            fs: FileSystem<Arc<dyn crate::FileSystem>, Vec<Box<dyn crate::FileSystem>>>,
        ) -> Arc<dyn crate::FileSystem + 'static> {
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

        let overlay = FileSystem::new(primary, [secondary]);

        // Delete a folder on the primary filesystem
        overlay.remove_dir(first).unwrap();
        assert_eq!(
            overlay.primary().metadata(first).unwrap_err(),
            FsError::EntryNotFound,
            "Deleted from primary"
        );
        assert!(!overlay.secondaries[0].exists(&second));

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
        assert!(overlay.secondaries[0].exists(third));
    }

    #[tokio::test]
    async fn open_files() {
        let primary = MemFS::default();
        let secondary = MemFS::default();
        primary.create_dir_all("/primary").unwrap();
        primary.touch("/primary/read.txt").unwrap();
        primary.touch("/primary/write.txt").unwrap();
        secondary.create_dir_all("/secondary").unwrap();
        secondary.touch("/secondary/read.txt").unwrap();
        secondary.touch("/secondary/write.txt").unwrap();
        secondary.create_dir_all("/primary").unwrap();
        secondary
            .write("/primary/read.txt", "This is shadowed")
            .await
            .unwrap();

        let fs = FileSystem::new(primary, [secondary]);

        // Any new files will be created on the primary fs
        let _ = fs
            .new_open_options()
            .create(true)
            .write(true)
            .open("/new.txt")
            .unwrap();
        assert!(fs.primary.exists("/new.txt"));
        assert!(!fs.secondaries[0].exists("/new.txt"));

        // You can open a file for reading and writing on the primary fs
        let _ = fs
            .new_open_options()
            .create(false)
            .write(true)
            .read(true)
            .open("/primary/write.txt")
            .unwrap();

        // Files on the primary should always shadow the secondary
        let content = fs.read_to_string("/primary/read.txt").await.unwrap();
        assert_ne!(content, "This is shadowed");
    }

    #[tokio::test]
    async fn listed_files_appear_overlayed() {
        let primary = MemFS::default();
        let secondary = MemFS::default();
        let secondary_overlayed = MemFS::default();
        primary.create_dir_all("/primary").unwrap();
        primary.touch("/primary/read.txt").unwrap();
        primary.touch("/primary/write.txt").unwrap();
        secondary.create_dir_all("/secondary").unwrap();
        secondary.touch("/secondary/read.txt").unwrap();
        secondary.touch("/secondary/write.txt").unwrap();
        // This second "secondary" filesystem should share the same folders as
        // the first one.
        secondary_overlayed.create_dir_all("/secondary").unwrap();
        secondary_overlayed
            .touch("/secondary/overlayed.txt")
            .unwrap();

        let fs = FileSystem::new(primary, [secondary, secondary_overlayed]);

        let paths: Vec<_> = fs.walk("/").map(|entry| entry.path()).collect();
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
}
