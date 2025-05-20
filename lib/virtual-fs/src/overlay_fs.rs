use std::{
    collections::HashSet,
    fmt::Debug,
    io::{self, SeekFrom},
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures::future::BoxFuture;
use replace_with::replace_with_or_abort;
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf};

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
///
/// let runtime = tokio::runtime::Builder::new_current_thread()
///     .enable_all()
///     .build()
///     .unwrap();
/// let _guard = runtime.enter();
///
/// let fs = OverlayFileSystem::new(MemFS::default(), [HostFS::new(tokio::runtime::Handle::current(), "/").unwrap()]);
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
    primary: Arc<P>,
    secondaries: S,
}

impl<P, S> OverlayFileSystem<P, S>
where
    P: FileSystem + Send + Sync + 'static,
    S: for<'a> FileSystems<'a> + Send + Sync + 'static,
{
    /// Create a new [`FileSystem`] using a primary [`crate::FileSystem`] and a
    /// chain of secondary [`FileSystems`].
    pub fn new(primary: P, secondaries: S) -> Self {
        OverlayFileSystem {
            primary: Arc::new(primary),
            secondaries,
        }
    }

    /// Get a reference to the primary filesystem.
    pub fn primary(&self) -> &P {
        &self.primary
    }

    /// Get a reference to the secondary filesystems.
    pub fn secondaries(&self) -> &S {
        &self.secondaries
    }

    /// Get a mutable reference to the secondary filesystems.
    pub fn secondaries_mut(&mut self) -> &mut S {
        &mut self.secondaries
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
    P: FileSystem + Send + 'static,
    S: for<'a> FileSystems<'a> + Send + Sync + 'static,
    for<'a> <<S as FileSystems<'a>>::Iter as IntoIterator>::IntoIter: Send,
{
    fn readlink(&self, path: &Path) -> crate::Result<PathBuf> {
        // Whiteout files can not be read, they are just markers
        if ops::is_white_out(path).is_some() {
            return Err(FsError::EntryNotFound);
        }

        // Check if the file is in the primary
        match self.primary.readlink(path) {
            Ok(meta) => return Ok(meta),
            Err(e) if should_continue(e) => {}
            Err(e) => return Err(e),
        }

        // There might be a whiteout, search for this
        if ops::has_white_out(&self.primary, path) {
            return Err(FsError::EntryNotFound);
        }

        // Otherwise scan the secondaries
        for fs in self.secondaries.filesystems() {
            match fs.readlink(path) {
                Err(e) if should_continue(e) => continue,
                other => return other,
            }
        }

        Err(FsError::EntryNotFound)
    }

    fn read_dir(&self, path: &Path) -> Result<ReadDir, FsError> {
        let mut entries = Vec::new();
        let mut had_at_least_one_success = false;
        let mut white_outs = HashSet::new();

        let filesystems = std::iter::once(&self.primary as &(dyn FileSystem + Send))
            .chain(self.secondaries().filesystems());

        for fs in filesystems {
            match fs.read_dir(path) {
                Ok(r) => {
                    for entry in r {
                        let entry = entry?;

                        // White out entries block any later entries in the secondaries
                        // unless the entry has comes before the white out, thus the order
                        // that the file systems are parsed is important to this logic.
                        if let Some(path) = entry.is_white_out() {
                            tracing::trace!(
                                path=%path.display(),
                                "Found whiteout file",
                            );
                            white_outs.insert(path);
                            continue;
                        } else if white_outs.contains(&entry.path) {
                            tracing::trace!(
                                path=%path.display(),
                                "Skipping path because a whiteout exists",
                            );
                            continue;
                        }

                        entries.push(entry);
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
        // You can not create directories that use the whiteout prefix
        if ops::is_white_out(path).is_some() {
            return Err(FsError::InvalidInput);
        }

        // It could be the case that the directory was earlier hidden in the secondaries
        // by a whiteout file, hence we need to make sure those are cleared out.
        ops::remove_white_out(self.primary.as_ref(), path);

        // Make sure the parent tree is in place on the primary, this is to cover the
        // scenario where the secondaries has a parent structure that is not yet in the
        // primary and the primary needs it to create a sub-directory
        if let Some(parent) = path.parent() {
            if self.read_dir(parent).is_ok() {
                ops::create_dir_all(&self.primary, parent).ok();
            }
        }

        // Create the directory in the primary
        match self.primary.create_dir(path) {
            Err(e) if should_continue(e) => {}
            other => return other,
        }

        self.permission_error_or_not_found(path)
    }

    fn remove_dir(&self, path: &Path) -> Result<(), FsError> {
        // Whiteout files can not be removed, instead the original directory
        // must be removed or recreated.
        if ops::is_white_out(path).is_some() {
            tracing::trace!(
                path=%path.display(),
                "Unable to remove a whited out directory",
            );
            return Err(FsError::EntryNotFound);
        }

        // If the directory is contained in a secondary file system then we need to create a
        // whiteout file so that it is suppressed and is no longer returned in `readdir` calls.

        let had_at_least_one_success = self.secondaries.filesystems().into_iter().any(|fs| {
            fs.read_dir(path).is_ok() && ops::create_white_out(&self.primary, path).is_ok()
        });

        // Attempt to remove it from the primary, if this succeeds then we may have also
        // added the whiteout file in the earlier step, but are required in this case to
        // properly delete the directory.
        match self.primary.remove_dir(path) {
            Err(e) if should_continue(e) => {}
            other => return other,
        }

        if had_at_least_one_success {
            return Ok(());
        }
        self.permission_error_or_not_found(path)
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<(), FsError>> {
        let from = from.to_owned();
        let to = to.to_owned();
        Box::pin(async move {
            // Whiteout files can not be renamed
            if ops::is_white_out(&from).is_some() {
                tracing::trace!(
                    from=%from.display(),
                    to=%to.display(),
                    "Attempting to rename a file that was whited out"
                );
                return Err(FsError::EntryNotFound);
            }
            // You can not rename a file into a whiteout file
            if ops::is_white_out(&to).is_some() {
                tracing::trace!(
                    from=%from.display(),
                    to=%to.display(),
                    "Attempting to rename a file into a whiteout file"
                );
                return Err(FsError::InvalidInput);
            }

            // We attempt to rename the file or directory in the primary
            // if this succeeds then we also need to ensure the white out
            // files are created where we need them, so we do not immediately
            // return until that is done
            let mut had_at_least_one_success = false;
            match self.primary.rename(&from, &to).await {
                Err(e) if should_continue(e) => {}
                Ok(()) => {
                    had_at_least_one_success = true;
                }
                other => return other,
            }

            // If we have not yet renamed the file it may still reside in
            // the secondaries, in which case we need to copy it to the
            // primary rather than rename it
            if !had_at_least_one_success {
                for fs in self.secondaries.filesystems() {
                    if fs.metadata(&from).is_ok() {
                        ops::copy_reference_ext(fs, &self.primary, &from, &to).await?;
                        had_at_least_one_success = true;
                        break;
                    }
                }
            }

            // If the rename operation was a success then we need to update any
            // whiteout files on the primary before we return success.
            if had_at_least_one_success {
                for fs in self.secondaries.filesystems() {
                    if fs.metadata(&from).is_ok() {
                        tracing::trace!(
                            path=%from.display(),
                            "Creating a whiteout for the file that was renamed",
                        );
                        ops::create_white_out(&self.primary, &from).ok();
                        break;
                    }
                }
                ops::remove_white_out(&self.primary, &to);
                return Ok(());
            }

            // Otherwise we are in a failure scenario
            self.permission_error_or_not_found(&from)
        })
    }

    fn metadata(&self, path: &Path) -> Result<Metadata, FsError> {
        // Whiteout files can not be read, they are just markers
        if ops::is_white_out(path).is_some() {
            return Err(FsError::EntryNotFound);
        }

        // Check if the file is in the primary
        match self.primary.metadata(path) {
            Ok(meta) => return Ok(meta),
            Err(e) if should_continue(e) => {}
            Err(e) => return Err(e),
        }

        // There might be a whiteout, search for this
        if ops::has_white_out(&self.primary, path) {
            return Err(FsError::EntryNotFound);
        }

        // Otherwise scan the secondaries
        for fs in self.secondaries.filesystems() {
            match fs.metadata(path) {
                Err(e) if should_continue(e) => continue,
                other => return other,
            }
        }

        Err(FsError::EntryNotFound)
    }

    fn symlink_metadata(&self, path: &Path) -> crate::Result<Metadata> {
        // Whiteout files can not be read, they are just markers
        if ops::is_white_out(path).is_some() {
            return Err(FsError::EntryNotFound);
        }

        // Check if the file is in the primary
        match self.primary.symlink_metadata(path) {
            Ok(meta) => return Ok(meta),
            Err(e) if should_continue(e) => {}
            Err(e) => return Err(e),
        }

        // There might be a whiteout, search for this
        if ops::has_white_out(&self.primary, path) {
            return Err(FsError::EntryNotFound);
        }

        // Otherwise scan the secondaries
        for fs in self.secondaries.filesystems() {
            match fs.symlink_metadata(path) {
                Err(e) if should_continue(e) => continue,
                other => return other,
            }
        }

        Err(FsError::EntryNotFound)
    }

    fn remove_file(&self, path: &Path) -> Result<(), FsError> {
        // It is not possible to delete whiteout files directly, instead
        // one must delete the original file
        if ops::is_white_out(path).is_some() {
            return Err(FsError::InvalidInput);
        }

        // If the file is contained in a secondary then then we need to create a
        // whiteout file so that it is suppressed.
        let had_at_least_one_success = self.secondaries.filesystems().into_iter().any(|fs| {
            fs.metadata(path).is_ok() && ops::create_white_out(&self.primary, path).is_ok()
        });

        // Attempt to remove it from the primary
        match self.primary.remove_file(path) {
            Err(e) if should_continue(e) => {}
            other => return other,
        }

        if had_at_least_one_success {
            return Ok(());
        }
        self.permission_error_or_not_found(path)
    }

    fn new_open_options(&self) -> OpenOptions<'_> {
        OpenOptions::new(self)
    }

    fn mount(
        &self,
        _name: String,
        _path: &Path,
        _fs: Box<dyn FileSystem + Send + Sync>,
    ) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
}

impl<P, S> FileOpener for OverlayFileSystem<P, S>
where
    P: FileSystem + Send + 'static,
    S: for<'a> FileSystems<'a> + Send + Sync + 'static,
    for<'a> <<S as FileSystems<'a>>::Iter as IntoIterator>::IntoIter: Send,
{
    fn open(
        &self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>, FsError> {
        // Whiteout files can not be read, they are just markers
        if ops::is_white_out(path).is_some() {
            tracing::trace!(
                path=%path.display(),
                options=?conf,
                "Whiteout files can't be opened",
            );
            return Err(FsError::InvalidInput);
        }

        // Check if the file is in the primary (without actually creating it) as
        // when the file is in the primary it takes preference over any of file
        {
            let mut conf = conf.clone();
            conf.create = false;
            conf.create_new = false;
            match self.primary.new_open_options().options(conf).open(path) {
                Err(e) if should_continue(e) => {}
                other => return other,
            }
        }

        // In the scenario that we are creating the file then there is
        // special handling that will ensure its setup correctly
        if conf.create_new {
            // When the secondary has the directory structure but the primary
            // does not then we need to make sure we create all the structure
            // in the primary
            if let Some(parent) = path.parent() {
                if ops::exists(self, parent) {
                    // We create the directory structure on the primary so that
                    // the new file can be created, this will make it override
                    // whatever is in the primary
                    ops::create_dir_all(&self.primary, parent)?;
                } else {
                    return Err(FsError::EntryNotFound);
                }
            }

            // Remove any whiteout
            ops::remove_white_out(&self.primary, path);

            // Create the file in the primary
            return self
                .primary
                .new_open_options()
                .options(conf.clone())
                .open(path);
        }

        // There might be a whiteout, search for this and if its found then
        // we are done as the secondary file or directory has been earlier
        // deleted via a white out (when the create flag is set then
        // the white out marker is ignored)
        if !conf.create && ops::has_white_out(&self.primary, path) {
            tracing::trace!(
                path=%path.display(),
                "The file has been whited out",
            );
            return Err(FsError::EntryNotFound);
        }

        // Determine if a mutation will be possible with the opened file
        let require_mutations = conf.append || conf.write || conf.create_new | conf.truncate;

        // If the file is on a secondary then we should open it
        if !ops::has_white_out(&self.primary, path) {
            for fs in self.secondaries.filesystems() {
                let mut sub_conf = conf.clone();
                sub_conf.create = false;
                sub_conf.create_new = false;
                sub_conf.append = false;
                sub_conf.truncate = false;
                match fs.new_open_options().options(sub_conf.clone()).open(path) {
                    Err(e) if should_continue(e) => continue,
                    Ok(file) if require_mutations => {
                        // If the file was opened with the ability to mutate then we need
                        // to return a copy on write emulation so that the file can be
                        // copied from the secondary to the primary in the scenario that
                        // it is edited
                        return open_copy_on_write(path, conf, &self.primary, file);
                    }
                    other => return other,
                }
            }
        }

        // If we are creating the file then do so
        if conf.create {
            // Create the parent structure and remove any whiteouts
            if let Some(parent) = path.parent() {
                if ops::exists(self, parent) {
                    ops::create_dir_all(&self.primary, parent)?;
                }
            }
            ops::remove_white_out(&self.primary, path);

            // Create the file in the primary
            return self
                .primary
                .new_open_options()
                .options(conf.clone())
                .open(path);
        }

        // The file does not exist anywhere
        Err(FsError::EntryNotFound)
    }
}

fn open_copy_on_write<P>(
    path: &Path,
    conf: &OpenOptionsConfig,
    primary: &Arc<P>,
    file: Box<dyn VirtualFile + Send + Sync>,
) -> Result<Box<dyn VirtualFile + Send + Sync>, FsError>
where
    P: FileSystem,
{
    struct CopyOnWriteFile<P> {
        path: PathBuf,
        primary: Arc<P>,
        state: CowState,
        readable: bool,
        append: bool,
        new_size: Option<u64>,
    }
    enum CowState {
        // The original file is still open and can be accessed for all
        // read operations
        ReadOnly(Box<dyn VirtualFile + Send + Sync>),
        // The copy has started but first we need to get the cursor
        // position within the source file so that it can be restored
        SeekingGet(Box<dyn VirtualFile + Send + Sync>),
        // Now we have the original starting cursor location we need
        // to move the position of the read to the start of the source
        // file
        SeekingSet {
            original_offset: u64,
            src: Box<dyn VirtualFile + Send + Sync>,
        },
        // We are now copying the data in parts held in the buffer piece
        // by piece until the original file is completely copied
        Copying {
            original_offset: u64,
            buf: Vec<u8>,
            buf_pos: usize,
            dst: Box<dyn VirtualFile + Send + Sync>,
            src: Box<dyn VirtualFile + Send + Sync>,
        },
        // After copying the file we need to seek the position back
        // to its original location on the newly copied file
        SeekingRestore {
            dst: Box<dyn VirtualFile + Send + Sync>,
        },
        // We have copied the file and can use all the normal operations
        Copied(Box<dyn VirtualFile + Send + Sync>),
        // An error occurred during the copy operation and we are now in a
        // failed state, after the error is consumed it will reset back
        // to the original file
        Error {
            err: io::Error,
            src: Box<dyn VirtualFile + Send + Sync>,
        },
    }
    impl CowState {
        fn as_ref(&self) -> &(dyn VirtualFile + Send + Sync) {
            match self {
                Self::ReadOnly(inner) => inner.as_ref(),
                Self::SeekingGet(inner) => inner.as_ref(),
                Self::SeekingSet { src, .. } => src.as_ref(),
                Self::Copying { src, .. } => src.as_ref(),
                Self::SeekingRestore { dst, .. } => dst.as_ref(),
                Self::Copied(inner) => inner.as_ref(),
                Self::Error { src, .. } => src.as_ref(),
            }
        }
        fn as_mut(&mut self) -> &mut (dyn VirtualFile + Send + Sync) {
            match self {
                Self::ReadOnly(inner) => inner.as_mut(),
                Self::SeekingGet(inner) => inner.as_mut(),
                Self::SeekingSet { src, .. } => src.as_mut(),
                Self::Copying { src, .. } => src.as_mut(),
                Self::SeekingRestore { dst, .. } => dst.as_mut(),
                Self::Copied(inner) => inner.as_mut(),
                Self::Error { src, .. } => src.as_mut(),
            }
        }
    }

    impl<P> CopyOnWriteFile<P>
    where
        P: FileSystem + 'static,
    {
        fn poll_copy_progress(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
            // Enter a loop until we go pending
            let mut again = true;
            while again {
                again = false;

                // The state machine is updated during the poll operation
                replace_with_or_abort(&mut self.state, |state| match state {
                    // We record the current position of the file so that it can be
                    // restored after the copy-on-write is finished
                    CowState::SeekingGet(mut src) => {
                        match Pin::new(src.as_mut()).poll_complete(cx) {
                            Poll::Ready(Ok(offset)) => {
                                if let Err(err) =
                                    Pin::new(src.as_mut()).start_seek(SeekFrom::Start(0))
                                {
                                    return CowState::Error { err, src };
                                }
                                again = true;
                                CowState::SeekingSet {
                                    original_offset: offset,
                                    src,
                                }
                            }
                            Poll::Ready(Err(err)) => CowState::Error { err, src },
                            Poll::Pending => CowState::SeekingGet(src),
                        }
                    }

                    // We complete the seek operation to the start of the source file
                    CowState::SeekingSet {
                        original_offset,
                        mut src,
                    } => {
                        match Pin::new(src.as_mut()).poll_complete(cx).map_ok(|_| ()) {
                            Poll::Ready(Ok(())) => {
                                // Remove the whiteout, create the parent structure and open
                                // the new file on the primary
                                if let Some(parent) = self.path.parent() {
                                    ops::create_dir_all(&self.primary, parent).ok();
                                }
                                let mut had_white_out = false;
                                if ops::has_white_out(&self.primary, &self.path) {
                                    ops::remove_white_out(&self.primary, &self.path);
                                    had_white_out = true;
                                }
                                let dst = self
                                    .primary
                                    .new_open_options()
                                    .create(true)
                                    .read(self.readable)
                                    .write(true)
                                    .truncate(true)
                                    .open(&self.path);
                                match dst {
                                    Ok(dst) if had_white_out => {
                                        again = true;
                                        CowState::Copied(dst)
                                    }
                                    Ok(dst) => {
                                        again = true;
                                        CowState::Copying {
                                            original_offset,
                                            buf: Vec::new(),
                                            buf_pos: 0,
                                            src,
                                            dst,
                                        }
                                    }
                                    Err(err) => CowState::Error {
                                        err: err.into(),
                                        src,
                                    },
                                }
                            }
                            Poll::Ready(Err(err)) => CowState::Error { err, src },
                            Poll::Pending => CowState::SeekingSet {
                                original_offset,
                                src,
                            },
                        }
                    }
                    // We are now copying all the data on blocks
                    CowState::Copying {
                        mut src,
                        mut dst,
                        mut buf,
                        mut buf_pos,
                        original_offset,
                    } => {
                        loop {
                            // We are either copying more data from the source
                            // or we are copying the data to the destination
                            if buf_pos < buf.len() {
                                let dst_pinned = Pin::new(dst.as_mut());
                                match dst_pinned.poll_write(cx, &buf[buf_pos..]) {
                                    Poll::Ready(Ok(0)) => {}
                                    Poll::Ready(Ok(amt)) => {
                                        buf_pos += amt;
                                        continue;
                                    }
                                    Poll::Ready(Err(err)) => {
                                        return CowState::Error { err, src };
                                    }
                                    Poll::Pending => {}
                                }
                            } else {
                                buf.resize_with(8192, || 0);
                                buf_pos = 8192;
                                let mut read_buf = ReadBuf::new(&mut buf);
                                match Pin::new(src.as_mut()).poll_read(cx, &mut read_buf) {
                                    Poll::Ready(Ok(())) if read_buf.filled().is_empty() => {
                                        again = true;

                                        if self.append {
                                            // When we append then we leave the cursor at the
                                            // end of the file
                                            return CowState::Copied(dst);
                                        } else {
                                            // No more data exists to be read so we now move on to
                                            // restoring the cursor back to the original position
                                            if let Err(err) = Pin::new(dst.as_mut())
                                                .start_seek(SeekFrom::Start(original_offset))
                                            {
                                                return CowState::Error { err, src };
                                            }
                                            return CowState::SeekingRestore { dst };
                                        }
                                    }
                                    Poll::Ready(Ok(())) => {
                                        // There is more data to be processed
                                        let new_len = read_buf.filled().len();
                                        unsafe { buf.set_len(new_len) };
                                        buf_pos = 0;
                                        continue;
                                    }
                                    Poll::Ready(Err(err)) => return CowState::Error { err, src },
                                    Poll::Pending => {}
                                }
                            }
                            return CowState::Copying {
                                original_offset,
                                buf,
                                buf_pos,
                                src,
                                dst,
                            };
                        }
                    }
                    // Now once the restoration of the seek position completes we set the copied state
                    CowState::SeekingRestore { mut dst } => {
                        match Pin::new(dst.as_mut()).poll_complete(cx) {
                            Poll::Ready(_) => {
                                // If we have changed the length then set it
                                if let Some(new_size) = self.new_size.take() {
                                    dst.set_len(new_size).ok();
                                }
                                CowState::Copied(dst)
                            }
                            Poll::Pending => CowState::SeekingRestore { dst },
                        }
                    }
                    s => s,
                });
            }

            // Determine what response to give based off the state, when an error occurs
            // this will be returned and the copy-on-write will be reset
            let mut ret = Poll::Pending;
            replace_with_or_abort(&mut self.state, |state| match state {
                CowState::ReadOnly(src) => {
                    ret = Poll::Ready(Ok(()));
                    CowState::ReadOnly(src)
                }
                CowState::Copied(src) => {
                    ret = Poll::Ready(Ok(()));
                    CowState::Copied(src)
                }
                CowState::Error { err, src } => {
                    ret = Poll::Ready(Err(err));
                    CowState::ReadOnly(src)
                }
                state => {
                    ret = Poll::Pending;
                    state
                }
            });
            ret
        }

        fn poll_copy_start_and_progress(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
            replace_with_or_abort(&mut self.state, |state| match state {
                CowState::ReadOnly(inner) => {
                    tracing::trace!("COW file touched, starting file clone");
                    CowState::SeekingGet(inner)
                }
                state => state,
            });
            self.poll_copy_progress(cx)
        }
    }

    impl<P> Debug for CopyOnWriteFile<P>
    where
        P: FileSystem + 'static,
    {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("CopyOnWriteFile").finish()
        }
    }

    impl<P> VirtualFile for CopyOnWriteFile<P>
    where
        P: FileSystem + 'static,
    {
        fn last_accessed(&self) -> u64 {
            self.state.as_ref().last_accessed()
        }

        fn last_modified(&self) -> u64 {
            self.state.as_ref().last_modified()
        }

        fn created_time(&self) -> u64 {
            self.state.as_ref().created_time()
        }

        fn size(&self) -> u64 {
            self.state.as_ref().size()
        }

        fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
            self.new_size = Some(new_size);
            replace_with_or_abort(&mut self.state, |state| match state {
                CowState::Copied(mut file) => {
                    file.set_len(new_size).ok();
                    CowState::Copied(file)
                }
                state => {
                    // in the scenario where the length is set but the file is not
                    // polled then we need to make sure we create a file properly
                    if let Some(parent) = self.path.parent() {
                        ops::create_dir_all(&self.primary, parent).ok();
                    }
                    let dst = self
                        .primary
                        .new_open_options()
                        .create(true)
                        .write(true)
                        .open(&self.path);
                    if let Ok(mut file) = dst {
                        file.set_len(new_size).ok();
                    }
                    state
                }
            });
            Ok(())
        }

        fn unlink(&mut self) -> crate::Result<()> {
            let primary = self.primary.clone();
            let path = self.path.clone();

            // Create the whiteout file in the primary
            let mut had_at_least_one_success = false;
            if ops::create_white_out(&primary, &path).is_ok() {
                had_at_least_one_success = true;
            }

            // Attempt to remove it from the primary first
            match primary.remove_file(&path) {
                Err(e) if should_continue(e) => {}
                other => return other,
            }

            if had_at_least_one_success {
                return Ok(());
            }
            Err(FsError::PermissionDenied)
        }

        fn poll_read_ready(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<std::io::Result<usize>> {
            match self.poll_copy_progress(cx) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => return Poll::Pending,
            }
            Pin::new(self.state.as_mut()).poll_read_ready(cx)
        }

        fn poll_write_ready(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<std::io::Result<usize>> {
            match self.poll_copy_progress(cx) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => return Poll::Pending,
            }
            Pin::new(self.state.as_mut()).poll_write_ready(cx)
        }
    }

    impl<P> AsyncWrite for CopyOnWriteFile<P>
    where
        P: FileSystem + 'static,
    {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> Poll<Result<usize, std::io::Error>> {
            match self.poll_copy_start_and_progress(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Ready(Ok(())) => {}
            }
            Pin::new(self.state.as_mut()).poll_write(cx, buf)
        }

        fn poll_write_vectored(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bufs: &[io::IoSlice<'_>],
        ) -> Poll<Result<usize, io::Error>> {
            match self.poll_copy_start_and_progress(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Ready(Ok(())) => {}
            }
            Pin::new(self.state.as_mut()).poll_write_vectored(cx, bufs)
        }

        fn poll_flush(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            match self.poll_copy_progress(cx) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => return Poll::Pending,
            }
            Pin::new(self.state.as_mut()).poll_flush(cx)
        }

        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            match self.poll_copy_progress(cx) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => return Poll::Pending,
            }
            Pin::new(self.state.as_mut()).poll_shutdown(cx)
        }
    }

    impl<P> AsyncRead for CopyOnWriteFile<P>
    where
        P: FileSystem + 'static,
    {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            match self.poll_copy_progress(cx) {
                Poll::Ready(Ok(())) => {}
                p => return p,
            }
            Pin::new(self.state.as_mut()).poll_read(cx, buf)
        }
    }

    impl<P> AsyncSeek for CopyOnWriteFile<P>
    where
        P: FileSystem + 'static,
    {
        fn start_seek(
            mut self: Pin<&mut Self>,
            position: std::io::SeekFrom,
        ) -> std::io::Result<()> {
            match &mut self.state {
                CowState::ReadOnly(file)
                | CowState::SeekingGet(file)
                | CowState::Error { src: file, .. }
                | CowState::Copied(file)
                | CowState::SeekingRestore { dst: file, .. } => {
                    Pin::new(file.as_mut()).start_seek(position)
                }
                CowState::SeekingSet {
                    original_offset,
                    src,
                    ..
                }
                | CowState::Copying {
                    original_offset,
                    src,
                    ..
                } => {
                    *original_offset = match position {
                        SeekFrom::Current(delta) => original_offset
                            .checked_add_signed(delta)
                            .unwrap_or(*original_offset),
                        SeekFrom::Start(pos) => pos,
                        SeekFrom::End(pos) => src
                            .size()
                            .checked_add_signed(pos)
                            .unwrap_or(*original_offset),
                    };
                    Ok(())
                }
            }
        }

        fn poll_complete(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<std::io::Result<u64>> {
            match self.poll_copy_progress(cx) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => return Poll::Pending,
            }
            Pin::new(self.state.as_mut()).poll_complete(cx)
        }
    }

    tracing::trace!(
        path=%path.display(),
        options=?conf,
        "Opening the file in copy-on-write mode",
    );
    Ok(Box::new(CopyOnWriteFile::<P> {
        path: path.to_path_buf(),
        primary: primary.clone(),
        state: CowState::ReadOnly(file),
        readable: conf.read,
        append: conf.append,
        new_size: None,
    }))
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
    use std::path::PathBuf;

    use super::*;
    use crate::mem_fs::FileSystem as MemFS;
    use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

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
        assert_eq!(overlay.remove_dir(third), Ok(()));

        // It should no longer exist
        assert_eq!(overlay.metadata(third).unwrap_err(), FsError::EntryNotFound);

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

    #[tokio::test]
    async fn create_file_that_looks_like_it_is_in_a_secondary_filesystem_folder() {
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
    async fn open_secondary_fs_files_in_write_mode() {
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
        f.seek(SeekFrom::Start(0)).await.unwrap();
        // next we will write a new set of bytes
        f.set_len(0).unwrap();
        assert_eq!(f.write(b"Hi").await.unwrap(), 2);
        // Same with flushing
        assert_eq!(f.flush().await.unwrap(), ());

        // if we now read it then the data should be different
        buf = String::new();
        f.seek(SeekFrom::Start(0)).await.unwrap();
        f.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, "Hi");
        drop(f);

        // including if we open it again
        let mut f = fs
            .new_open_options()
            .read(true)
            .open("/secondary/file.txt")
            .unwrap();
        buf = String::new();
        f.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, "Hi");
    }

    #[tokio::test]
    async fn open_secondary_fs_files_unlink() {
        let primary = MemFS::default();
        let secondary = MemFS::default();
        ops::create_dir_all(&secondary, "/secondary").unwrap();
        ops::write(&secondary, "/secondary/file.txt", b"Hello, World!")
            .await
            .unwrap();

        let fs = OverlayFileSystem::new(primary, [secondary]);

        fs.metadata(Path::new("/secondary/file.txt")).unwrap();

        // Now delete the file and make sure its not found
        fs.remove_file(Path::new("/secondary/file.txt")).unwrap();
        assert_eq!(
            fs.metadata(Path::new("/secondary/file.txt")).unwrap_err(),
            FsError::EntryNotFound
        )
    }

    #[tokio::test]
    async fn open_secondary_fs_without_cow() {
        let primary = MemFS::default();
        let secondary = MemFS::default();
        ops::create_dir_all(&secondary, "/secondary").unwrap();
        ops::write(&secondary, "/secondary/file.txt", b"Hello, World!")
            .await
            .unwrap();

        let fs = OverlayFileSystem::new(primary, [secondary]);

        let mut f = fs
            .new_open_options()
            .create(true)
            .read(true)
            .open(Path::new("/secondary/file.txt"))
            .unwrap();
        assert_eq!(f.size() as usize, 13);

        let mut buf = String::new();
        f.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, "Hello, World!");

        // it should not be in the primary and nor should the secondary folder
        assert!(!ops::is_dir(&fs.primary, "/secondary"));
        assert!(!ops::is_file(&fs.primary, "/secondary/file.txt"));
        assert!(ops::is_dir(&fs.secondaries[0], "/secondary"));
        assert!(ops::is_file(&fs.secondaries[0], "/secondary/file.txt"));
    }

    #[tokio::test]
    async fn create_and_append_secondary_fs_with_cow() {
        let primary = MemFS::default();
        let secondary = MemFS::default();
        ops::create_dir_all(&secondary, "/secondary").unwrap();
        ops::write(&secondary, "/secondary/file.txt", b"Hello, World!")
            .await
            .unwrap();

        let fs = OverlayFileSystem::new(primary, [secondary]);

        let mut f = fs
            .new_open_options()
            .create(true)
            .append(true)
            .read(true)
            .open(Path::new("/secondary/file.txt"))
            .unwrap();
        assert_eq!(f.size() as usize, 13);

        f.write_all(b"asdf").await.unwrap();
        assert_eq!(f.size() as usize, 17);

        f.seek(SeekFrom::Start(0)).await.unwrap();

        let mut buf = String::new();
        f.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, "Hello, World!asdf");

        // Now lets check the file systems under
        let f = fs
            .primary
            .new_open_options()
            .create(true)
            .append(true)
            .read(true)
            .open(Path::new("/secondary/file.txt"))
            .unwrap();
        assert_eq!(f.size() as usize, 17);
        let f = fs.secondaries[0]
            .new_open_options()
            .create(true)
            .append(true)
            .read(true)
            .open(Path::new("/secondary/file.txt"))
            .unwrap();
        assert_eq!(f.size() as usize, 13);

        // it should now exist in both the primary and secondary
        assert!(ops::is_dir(&fs.primary, "/secondary"));
        assert!(ops::is_file(&fs.primary, "/secondary/file.txt"));
        assert!(ops::is_dir(&fs.secondaries[0], "/secondary"));
        assert!(ops::is_file(&fs.secondaries[0], "/secondary/file.txt"));
    }

    #[tokio::test]
    async fn unlink_file_from_secondary_fs() {
        let primary = MemFS::default();
        let secondary = MemFS::default();
        ops::create_dir_all(&secondary, "/secondary").unwrap();
        ops::write(&secondary, "/secondary/file.txt", b"Hello, World!")
            .await
            .unwrap();

        let fs = OverlayFileSystem::new(primary, [secondary]);

        fs.remove_file(Path::new("/secondary/file.txt")).unwrap();
        assert_eq!(ops::exists(&fs, Path::new("/secondary/file.txt")), false);

        assert!(ops::is_file(&fs.primary, "/secondary/.wh.file.txt"));
        assert!(ops::is_file(&fs.secondaries[0], "/secondary/file.txt"));

        // Now create the file again after the unlink
        let mut f = fs
            .new_open_options()
            .create(true)
            .write(true)
            .read(true)
            .open(Path::new("/secondary/file.txt"))
            .unwrap();
        assert_eq!(f.size() as usize, 0);
        f.write_all(b"asdf").await.unwrap();
        assert_eq!(f.size() as usize, 4);

        // The whiteout should be gone and new file exist
        assert!(!ops::is_file(&fs.primary, "/secondary/.wh.file.txt"));
        assert!(ops::is_file(&fs.primary, "/secondary/file.txt"));
        assert!(ops::is_file(&fs.secondaries[0], "/secondary/file.txt"));
    }

    #[tokio::test]
    async fn rmdir_from_secondary_fs() {
        let primary = MemFS::default();
        let secondary = MemFS::default();
        ops::create_dir_all(&secondary, "/secondary").unwrap();

        let fs = OverlayFileSystem::new(primary, [secondary]);

        assert!(ops::is_dir(&fs, "/secondary"));
        fs.remove_dir(Path::new("/secondary")).unwrap();

        assert!(!ops::is_dir(&fs, "/secondary"));
        assert!(ops::is_file(&fs.primary, "/.wh.secondary"));
        assert!(ops::is_dir(&fs.secondaries[0], "/secondary"));

        fs.create_dir(Path::new("/secondary")).unwrap();
        assert!(ops::is_dir(&fs, "/secondary"));
        assert!(ops::is_dir(&fs.primary, "/secondary"));
        assert!(!ops::is_file(&fs.primary, "/.wh.secondary"));
        assert!(ops::is_dir(&fs.secondaries[0], "/secondary"));
    }

    #[tokio::test]
    async fn rmdir_sub_from_secondary_fs() {
        let primary = MemFS::default();
        let secondary = MemFS::default();
        ops::create_dir_all(&secondary, "/first/secondary").unwrap();

        let fs = OverlayFileSystem::new(primary, [secondary]);

        assert!(ops::is_dir(&fs, "/first/secondary"));
        fs.remove_dir(Path::new("/first/secondary")).unwrap();

        assert!(!ops::is_dir(&fs, "/first/secondary"));
        assert!(ops::is_file(&fs.primary, "/first/.wh.secondary"));
        assert!(ops::is_dir(&fs.secondaries[0], "/first/secondary"));

        fs.create_dir(Path::new("/first/secondary")).unwrap();
        assert!(ops::is_dir(&fs, "/first/secondary"));
        assert!(ops::is_dir(&fs.primary, "/first/secondary"));
        assert!(!ops::is_file(&fs.primary, "/first/.wh.secondary"));
        assert!(ops::is_dir(&fs.secondaries[0], "/first/secondary"));
    }

    #[tokio::test]
    async fn create_new_secondary_fs_without_cow() {
        let primary = MemFS::default();
        let secondary = MemFS::default();
        ops::create_dir_all(&secondary, "/secondary").unwrap();
        ops::write(&secondary, "/secondary/file.txt", b"Hello, World!")
            .await
            .unwrap();

        let fs = OverlayFileSystem::new(primary, [secondary]);

        let mut f = fs
            .new_open_options()
            .create_new(true)
            .read(true)
            .open(Path::new("/secondary/file.txt"))
            .unwrap();
        assert_eq!(f.size() as usize, 0);

        let mut buf = String::new();
        f.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, "");

        // it should now exist in both the primary and secondary
        assert!(ops::is_dir(&fs.primary, "/secondary"));
        assert!(ops::is_file(&fs.primary, "/secondary/file.txt"));
        assert!(ops::is_dir(&fs.secondaries[0], "/secondary"));
        assert!(ops::is_file(&fs.secondaries[0], "/secondary/file.txt"));
    }

    #[tokio::test]
    async fn open_secondary_fs_files_remove_dir() {
        let primary = MemFS::default();
        let secondary = MemFS::default();
        ops::create_dir_all(&secondary, "/secondary").unwrap();

        let fs = OverlayFileSystem::new(primary, [secondary]);

        fs.metadata(Path::new("/secondary")).unwrap();

        // Now delete the file and make sure its not found
        fs.remove_dir(Path::new("/secondary")).unwrap();
        assert_eq!(
            fs.metadata(Path::new("/secondary")).unwrap_err(),
            FsError::EntryNotFound
        )
    }

    /// Make sure files that are never written are not copied to the primary,
    /// even when opened with write permissions.
    /// Regression test for https://github.com/wasmerio/wasmer/issues/5445
    #[tokio::test]
    async fn test_overlayfs_readonly_files_not_copied() {
        let primary = MemFS::default();
        let secondary = MemFS::default();
        ops::create_dir_all(&secondary, "/secondary").unwrap();
        ops::write(&secondary, "/secondary/file.txt", b"Hello, World!")
            .await
            .unwrap();

        let fs = OverlayFileSystem::new(primary, [secondary]);

        {
            let mut f = fs
                .new_open_options()
                .read(true)
                .write(true)
                .open(Path::new("/secondary/file.txt"))
                .unwrap();
            let mut s = String::new();
            f.read_to_string(&mut s).await.unwrap();
            assert_eq!(s, "Hello, World!");

            f.flush().await.unwrap();
            f.shutdown().await.unwrap();
        }

        // Primary should not have the file
        assert!(!ops::is_file(&fs.primary, "/secondary/file.txt"));
    }

    // OLD tests that used WebcFileSystem.
    // Should be re-implemented with WebcVolumeFs
    // #[tokio::test]
    // async fn wasi_runner_use_case() {
    //     // Set up some dummy files on the host
    //     let temp = TempDir::new().unwrap();
    //     let first = temp.path().join("first");
    //     let file_txt = first.join("file.txt");
    //     let second = temp.path().join("second");
    //     std::fs::create_dir_all(&first).unwrap();
    //     std::fs::write(&file_txt, b"First!").unwrap();
    //     std::fs::create_dir_all(&second).unwrap();
    //     // configure the union FS so things are saved in memory by default
    //     // (initialized with a set of unix-like folders), but certain folders
    //     // are first to the host.
    //     let primary = RootFileSystemBuilder::new().build();
    //     let host_fs: Arc<dyn FileSystem + Send + Sync> =
    //         Arc::new(crate::host_fs::FileSystem::default());
    //     let first_dirs = [(&first, "/first"), (&second, "/second")];
    //     for (host, guest) in first_dirs {
    //         primary
    //             .mount(PathBuf::from(guest), &host_fs, host.clone())
    //             .unwrap();
    //     }
    //     // Set up the secondary file systems
    //     let webc = WebCOwned::parse(Bytes::from_static(PYTHON), &ParseOptions::default()).unwrap();
    //     let webc = WebcFileSystem::init_all(Arc::new(webc));
    //
    //     let fs = OverlayFileSystem::new(primary, [webc]);
    //
    //     // We should get all the normal directories from rootfs (primary)
    //     assert!(ops::is_dir(&fs, "/lib"));
    //     assert!(ops::is_dir(&fs, "/bin"));
    //     assert!(ops::is_file(&fs, "/dev/stdin"));
    //     assert!(ops::is_file(&fs, "/dev/stdout"));
    //     // We also want to see files from the WEBC volumes (secondary)
    //     assert!(ops::is_dir(&fs, "/lib/python3.6"));
    //     assert!(ops::is_file(&fs, "/lib/python3.6/collections/__init__.py"));
    //     #[cfg(never)]
    //     {
    //         // files on a secondary fs aren't writable
    //         // TODO(Michael-F-Bryan): re-enable this if/when we fix
    //         // open_readonly_file_hack()
    //         assert_eq!(
    //             fs.new_open_options()
    //                 .append(true)
    //                 .open("/lib/python3.6/collections/__init__.py")
    //                 .unwrap_err(),
    //             FsError::PermissionDenied,
    //         );
    //     }
    //     // you are allowed to create files that look like they are in a secondary
    //     // folder, though
    //     ops::touch(&fs, "/lib/python3.6/collections/something-else.py").unwrap();
    //     // But it'll be on the primary filesystem, not the secondary one
    //     assert!(ops::is_file(
    //         &fs.primary,
    //         "/lib/python3.6/collections/something-else.py"
    //     ));
    //     assert!(!ops::is_file(
    //         &fs.secondaries[0],
    //         "/lib/python3.6/collections/something-else.py"
    //     ));
    //     // You can do the same thing with folders
    //     fs.create_dir("/lib/python3.6/something-else".as_ref())
    //         .unwrap();
    //     assert!(ops::is_dir(&fs.primary, "/lib/python3.6/something-else"));
    //     assert!(!ops::is_dir(
    //         &fs.secondaries[0],
    //         "/lib/python3.6/something-else"
    //     ));
    //     // It only works when you are directly inside an existing directory
    //     // on the secondary filesystem, though
    //     assert_eq!(
    //         ops::touch(&fs, "/lib/python3.6/collections/this/doesnt/exist.txt").unwrap_err(),
    //         FsError::EntryNotFound
    //     );
    //     // you should also be able to read files mounted from the host
    //     assert!(ops::is_dir(&fs, "/first"));
    //     assert!(ops::is_file(&fs, "/first/file.txt"));
    //     assert_eq!(
    //         ops::read_to_string(&fs, "/first/file.txt").await.unwrap(),
    //         "First!"
    //     );
    //     // Overwriting them is fine and we'll see the changes on the host
    //     ops::write(&fs, "/first/file.txt", "Updated").await.unwrap();
    //     assert_eq!(std::fs::read_to_string(&file_txt).unwrap(), "Updated");
    //     // The filesystem will see changes on the host that happened after it was
    //     // set up
    //     let another = second.join("another.txt");
    //     std::fs::write(&another, "asdf").unwrap();
    //     assert_eq!(
    //         ops::read_to_string(&fs, "/second/another.txt")
    //             .await
    //             .unwrap(),
    //         "asdf"
    //     );
    // }
    //
    // #[tokio::test]
    // async fn absolute_and_relative_paths_are_passed_through() {
    //     let python = Arc::new(load_webc(PYTHON));
    //
    //     // The underlying filesystem doesn't care about absolute/relative paths
    //     assert_eq!(python.read_dir("/lib".as_ref()).unwrap().count(), 4);
    //     assert_eq!(python.read_dir("lib".as_ref()).unwrap().count(), 4);
    //
    //     // read_dir() should be passed through to the primary
    //     let webc_primary =
    //         OverlayFileSystem::new(Arc::clone(&python), [crate::EmptyFileSystem::default()]);
    //     assert_same_directory_contents(&python, "/lib", &webc_primary);
    //     assert_same_directory_contents(&python, "lib", &webc_primary);
    //
    //     // read_dir() should also be passed through to the secondary
    //     let webc_secondary =
    //         OverlayFileSystem::new(crate::EmptyFileSystem::default(), [Arc::clone(&python)]);
    //     assert_same_directory_contents(&python, "/lib", &webc_secondary);
    //     assert_same_directory_contents(&python, "lib", &webc_secondary);
    //
    //     // It should be fine to overlay the root fs on top of our webc file
    //     let overlay_rootfs = OverlayFileSystem::new(
    //         RootFileSystemBuilder::default().build(),
    //         [Arc::clone(&python)],
    //     );
    //     assert_same_directory_contents(&python, "/lib", &overlay_rootfs);
    //     assert_same_directory_contents(&python, "lib", &overlay_rootfs);
    // }
    // #[track_caller]
    // fn assert_same_directory_contents(
    //     original: &dyn FileSystem,
    //     path: impl AsRef<Path>,
    //     candidate: &dyn FileSystem,
    // ) {
    //     let path = path.as_ref();
    //
    //     let original_entries: Vec<_> = original
    //         .read_dir(path)
    //         .unwrap()
    //         .map(|r| r.unwrap())
    //         .collect();
    //     let candidate_entries: Vec<_> = candidate
    //         .read_dir(path)
    //         .unwrap()
    //         .map(|r| r.unwrap())
    //         .collect();
    //
    //     assert_eq!(original_entries, candidate_entries);
    // }
}
