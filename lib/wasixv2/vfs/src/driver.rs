//! The interface filesystem drivers implement.
//!
//! Drivers store data. They never see paths, `.` or `..`, symlink resolution, credentials or
//! permission checks: the VFS walks paths one component at a time, decides every permission and
//! ownership question, and only then calls the driver. Mirrors the split between Linux's VFS and
//! a filesystem's `inode_operations`/`file_operations`.
//!
//! Everything is async so a driver can sit on the host's filesystem or the network. Futures are
//! not `Send`, like the syscalls that drive them.

use std::{any::Any, sync::Arc};

use async_trait::async_trait;
use wasixv2_abi::Errno;
use wasixv2_pages::SharedPages;
use wasixv2_wait::{PollMask, PollTable};

use crate::{
    cred::Cred,
    fileattr::FileKattr,
    mount::Path,
    ops::Ctx,
    posix_acl::PosixAcl,
    types::{
        AllocMode, Attr, DirSink, FileId, FileType, FsInfo, FsResult, NewNode, OpenOptions,
        RenameMode, SeekHole, SetAttr, StatFs, XattrSet,
    },
};

/// A mounted filesystem instance (Linux's `super_block`).
#[async_trait(?Send)]
pub trait Filesystem: Send + Sync {
    fn info(&self) -> FsInfo;

    /// The root directory. Available as soon as the filesystem is, so mounting needs no I/O.
    fn root(&self) -> Arc<dyn Node>;

    async fn statfs(&self) -> FsResult<StatFs>;

    /// Finishes any handshake that the filesystem requires before the mount is attached.
    async fn initialize(&self) -> FsResult<()> {
        Ok(())
    }

    /// Flushes everything to stable storage (`syncfs`).
    async fn sync(&self) -> FsResult<()> {
        Ok(())
    }

    /// Appends the filesystem's own options, each as `,key[=value]`, as `mountinfo`,
    /// `/proc/<pid>/mounts` and `statmount` show them (`show_options`). By default there are
    /// none.
    fn show_options(&self, _out: &mut Vec<u8>) {}

    /// The name `d_path` gives an object of this filesystem instead of its path (`d_dname`),
    /// for filesystems that are mounted nowhere, whose objects have no path: `pipe:[ino]`,
    /// `anon_inode:[eventfd]`. `name` is the name the object was given when its path was made.
    fn dname(&self, _name: &[u8], _node: &dyn Node) -> Option<Vec<u8>> {
        None
    }

    /// Whether the objects of this filesystem, which is mounted nowhere, may be bind-mounted
    /// although they have no name (`may_copy_tree` makes the exception for nsfs and pidfs):
    /// binding a namespace file keeps the namespace, as `ip netns add` does. By default not.
    fn binds_objects(&self) -> bool {
        false
    }

    /// `export_operations->encode_fh`: a handle for `node` that [`Filesystem::fh_to_node`]
    /// decodes, for `name_to_handle_at` and `open_by_handle_at`. `None` (the default) for a
    /// filesystem without export operations, whose objects only get identifiers that are
    /// never decoded (`AT_HANDLE_FID`, fanotify's), which the VFS makes from the inode number
    /// and generation (`exportfs_encode_ino64_fid`).
    fn encode_fh(&self, _node: &dyn Node) -> Option<FileId> {
        None
    }

    /// `export_operations->fh_to_dentry`: the object `fid`, which [`Filesystem::encode_fh`]
    /// made, while it exists; `ESTALE` once it is gone or for a handle that names nothing.
    async fn fh_to_node(&self, _fid: &FileId) -> FsResult<Arc<dyn Node>> {
        Err(Errno::ESTALE)
    }
}

/// Where following a symbolic link leads.
pub enum Link {
    /// A path to walk, as a symlink's body.
    Body(Vec<u8>),
    /// An object itself (`/proc/<pid>/fd/<n>`, `/proc/<pid>/cwd`): the walk continues from it
    /// without a path to walk, as Linux's `nd_jump_link` does for magic links. `readlink` shows
    /// its `d_path`.
    Jump(Path),
}

/// A filesystem object (Linux's `inode` plus its `inode_operations`).
///
/// Directory methods default to `ENOTDIR` and are only called on directories; names are single
/// components, never empty, `.` or `..`, never containing `/` or NUL, and at most
/// `FsInfo::name_max` bytes. `create`, `link` and `rename` get objects of the same filesystem.
///
/// Drivers keep the timestamps their own operations imply, as Linux filesystems do in their inode
/// operations: a write updates `mtime` and `ctime`, a new or removed entry updates the parent's,
/// `link`, `unlink` and `rename` update the `ctime` of the object whose link count or name
/// changed. Explicit changes come through `setattr`.
#[async_trait(?Send)]
pub trait Node: Send + Sync + Any {
    /// The inode number: stable for the object's lifetime and unique within the filesystem, and
    /// the object's identity. The VFS keeps one inode per number (its lock, its file locks, its
    /// directory entry), so two `Node`s with one number must be the same object, and a number
    /// must not name another object while a `Node` for the first is alive. A driver spanning
    /// several devices folds the device into the number.
    fn ino(&self) -> u64;

    fn file_type(&self) -> FileType;

    fn as_any(&self) -> &dyn Any;

    /// The `FS_*_FL` flags of the object the VFS enforces (`i_flags`' `S_IMMUTABLE` and
    /// `S_APPEND`), as [`Node::fileattr_get`] reports them. None by default.
    fn inode_flags(&self) -> u32 {
        0
    }

    /// `i_op->fileattr_get`: the object's miscellaneous attributes. `ENOTTY` (Linux's
    /// `ENOIOCTLCMD`) for a filesystem without them.
    async fn fileattr_get(&self) -> FsResult<FileKattr> {
        Err(Errno::ENOTTY)
    }

    /// `i_op->fileattr_set`: stores `fa`, which the VFS checked and completed from the
    /// current values; `EOPNOTSUPP` for what the filesystem cannot keep. Stamps `ctime`.
    async fn fileattr_set(&self, _fa: &FileKattr) -> FsResult<()> {
        Err(Errno::ENOTTY)
    }

    /// `i_generation`: tells apart objects that had the same inode number at different
    /// times, for file handles and `FS_IOC_GETVERSION`. 0 for a filesystem that never reuses
    /// a number.
    fn generation(&self) -> u32 {
        0
    }

    /// `inode->i_op->permission`: whether `cred`, of the task `task` (as [`Ctx::task`] numbers
    /// it), may access the object as `mask` asks, its metadata being `attr` and its access ACL
    /// `acl` (given for all but the owner while the group class has a permission). By default
    /// the mode bits and the ACL decide (`generic_permission`); a pseudo filesystem whose files are checked
    /// against the caller rather than their owner (procfs's sysctl files,
    /// `proc_sys_permission`) decides itself.
    ///
    /// [`Ctx::task`]: crate::Ctx::task
    fn permission(
        &self,
        cred: &Cred,
        _task: u32,
        attr: &Attr,
        acl: Option<&PosixAcl>,
        mask: u32,
    ) -> FsResult<()> {
        crate::generic_permission_acl(cred, attr, acl, mask)
    }

    /// Filesystems whose permission operation performs I/O override this; the default
    /// preserves the synchronous permission hook used by pseudo filesystems.
    async fn check_permission(
        &self,
        cred: &Cred,
        task: u32,
        attr: &Attr,
        acl: Option<&PosixAcl>,
        mask: u32,
    ) -> FsResult<()> {
        self.permission(cred, task, attr, acl, mask)
    }

    /// Nothing may be mounted on this object, as procfs refuses mounts on the descriptor links
    /// of `/proc/<pid>/fd` (`proc_splice_unmountable`): mounting there fails with `ENOENT`.
    fn unmountable(&self) -> bool {
        false
    }

    async fn getattr(&self) -> FsResult<Attr>;

    /// Applies `attr` and returns the resulting metadata. A size change on a regular file
    /// truncates or extends it.
    async fn setattr(&self, attr: &SetAttr) -> FsResult<Attr>;

    /// The entry `name`; `ENOENT` if there is none.
    async fn lookup(&self, _name: &[u8]) -> FsResult<Arc<dyn Node>> {
        Err(Errno::ENOTDIR)
    }

    /// Adds a new object under `name`; `EEXIST` if the name is taken.
    async fn create(&self, _name: &[u8], _new: &NewNode<'_>) -> FsResult<Arc<dyn Node>> {
        Err(Errno::ENOTDIR)
    }

    /// Adds a new regular file under `name` and opens it at once, as `open(O_CREAT)` does when
    /// it creates the file (Linux's `->atomic_open`, `FMODE_CREATED`): the creator gets the
    /// access `options` asks for whatever the new file's permission bits are. `EEXIST` if the
    /// name is taken. A driver whose `open` checks permission bits itself (a host directory)
    /// overrides this.
    async fn create_open(
        &self,
        name: &[u8],
        new: &NewNode<'_>,
        options: OpenOptions,
    ) -> FsResult<(Arc<dyn Node>, Box<dyn Handle>)> {
        let node = self.create(name, new).await?;
        let handle = node.open(options).await?;
        Ok((node, handle))
    }

    /// Creates an unnamed regular file in this directory for `O_TMPFILE` and opens it with
    /// `options`, which a new file grants whatever its permission bits (Linux's `->tmpfile`).
    /// Its link count is 0 until `link` gives it a name.
    async fn create_unnamed(
        &self,
        _new: &NewNode<'_>,
        _options: OpenOptions,
    ) -> FsResult<(Arc<dyn Node>, Box<dyn Handle>)> {
        Err(Errno::EOPNOTSUPP)
    }

    /// Adds `name` as another link to `target`, a non-directory.
    async fn link(&self, _name: &[u8], _target: &Arc<dyn Node>) -> FsResult<()> {
        Err(Errno::ENOTDIR)
    }

    /// Removes the non-directory entry `name`.
    async fn unlink(&self, _name: &[u8]) -> FsResult<()> {
        Err(Errno::ENOTDIR)
    }

    /// Removes the directory entry `name`; `ENOTEMPTY` unless it is empty.
    async fn rmdir(&self, _name: &[u8]) -> FsResult<()> {
        Err(Errno::ENOTDIR)
    }

    /// Moves entry `name` to `new_name` in `new_dir` (possibly `self`). The VFS has already
    /// checked that the source exists, that types match for a replacement, that a directory is
    /// not moved under itself, and that `Exchange` has a target. Replacing a directory requires
    /// it to be empty (`ENOTEMPTY`). Unsupported modes return `EINVAL`.
    async fn rename(
        &self,
        _name: &[u8],
        _new_dir: &Arc<dyn Node>,
        _new_name: &[u8],
        _mode: RenameMode,
    ) -> FsResult<()> {
        Err(Errno::ENOTDIR)
    }

    /// The target of a symbolic link.
    async fn readlink(&self) -> FsResult<Vec<u8>> {
        Err(Errno::EINVAL)
    }

    /// Where following this symbolic link leads for the caller `cx` (`get_link`). The default
    /// is the body `readlink` gives; pseudo filesystems answer per caller (`/proc/self`) or with
    /// an object (magic links).
    async fn get_link(&self, _cx: &Ctx<'_>) -> FsResult<Link> {
        self.readlink().await.map(Link::Body)
    }

    /// Opens a regular file or directory for data access.
    async fn open(&self, options: OpenOptions) -> FsResult<Box<dyn Handle>>;

    /// The shared memory object that holds this regular file's data, so that a `MAP_SHARED`
    /// mapping can show its pages instead of a copy, as every mapping of a tmpfs file shows the
    /// file's shmem pages (`shmem_mmap`). The object is made at least `end` bytes long and
    /// never shrinks below that while the node lives, whatever the file's size becomes, so a
    /// mapping up to `end` never reaches past it. `None` where the data lives elsewhere: such
    /// a file's mappings are copies.
    fn shared_pages(&self, _end: u64) -> FsResult<Option<Arc<dyn SharedPages>>> {
        Ok(None)
    }

    /// The value of extended attribute `name` (full name, including the namespace prefix);
    /// `ENODATA` if it is not set.
    async fn getxattr(&self, _name: &[u8]) -> FsResult<Vec<u8>> {
        Err(Errno::EOPNOTSUPP)
    }

    async fn setxattr(&self, _name: &[u8], _value: &[u8], _how: XattrSet) -> FsResult<()> {
        Err(Errno::EOPNOTSUPP)
    }

    /// Names of all extended attributes.
    async fn listxattr(&self) -> FsResult<Vec<Vec<u8>>> {
        Err(Errno::EOPNOTSUPP)
    }

    async fn removexattr(&self, _name: &[u8]) -> FsResult<()> {
        Err(Errno::EOPNOTSUPP)
    }
}

/// An object opened for data access (Linux's `file_operations`).
///
/// Data methods are called on regular files only and `readdir` on directories only.
#[async_trait(?Send)]
pub trait Handle: Send + Sync {
    /// Some filesystems query userspace before the synchronous readiness sample.
    async fn prepare_poll(&self, _events: u32) {}

    fn can_poll(&self) -> bool {
        false
    }

    /// The filesystem's private open-file state, for operations involving two handles.
    fn as_any(&self) -> Option<&dyn Any> {
        None
    }

    /// `file_operations.flush`, called for every closed descriptor, including duplicates.
    async fn flush(&self, _owner: u64) -> FsResult<()> {
        Ok(())
    }

    /// Validates a file mapping before allocating its pages (`file_operations.mmap`).
    fn mmap(&self, _shared: bool) -> FsResult<()> {
        Ok(())
    }

    /// Filesystem offload, or None to use the VFS's read/write fallback.
    async fn copy_range(
        &self,
        _offset: u64,
        _output: &dyn Handle,
        _output_offset: u64,
        _len: u64,
    ) -> FsResult<Option<u64>> {
        Ok(None)
    }

    /// Reads at `offset`; 0 at or past the end.
    async fn read_at(&self, _buf: &mut [u8], _offset: u64) -> FsResult<usize> {
        Err(Errno::EISDIR)
    }

    /// Writes at `offset`, extending the file (with a hole) if it starts past the end.
    async fn write_at(&self, _data: &[u8], _offset: u64) -> FsResult<usize> {
        Err(Errno::EISDIR)
    }

    /// Writes at the current end of the file atomically with finding it (`O_APPEND`). Returns
    /// the offset written at and the count.
    async fn append(&self, _data: &[u8]) -> FsResult<(u64, usize)> {
        Err(Errno::EISDIR)
    }

    /// Flushes data, and metadata unless `data_only` (`fsync`/`fdatasync`).
    async fn sync(&self, _data_only: bool) -> FsResult<()> {
        Ok(())
    }

    async fn allocate(&self, _mode: AllocMode, _offset: u64, _len: u64) -> FsResult<()> {
        Err(Errno::EOPNOTSUPP)
    }

    /// Truncates the file, or extends it with a hole, to `size` through this open file, as
    /// `ftruncate` does (Linux's `ATTR_FILE`): it needs the open file's write access, never the
    /// permission bits. Stamps `mtime` and `ctime`. Only called on handles opened for writing.
    async fn set_size(&self, _size: u64) -> FsResult<()> {
        Err(Errno::EINVAL)
    }

    /// The first offset at or after `offset` that starts data or a hole; `ENXIO` if `offset` is
    /// at or past the end. The default treats the whole file as data, as Linux's
    /// `generic_file_llseek` does.
    async fn seek_hole(&self, offset: u64, size: u64, which: SeekHole) -> FsResult<u64> {
        if offset >= size {
            return Err(Errno::ENXIO);
        }
        Ok(match which {
            SeekHole::Data => offset,
            SeekHole::Hole => size,
        })
    }

    /// The file's whole contents, when the driver keeps them in memory that never changes for
    /// as long as anyone holds them, such as a read-only image's: `execve` shares them instead
    /// of reading the file into a copy. `None` (the default) for everything else.
    async fn shared_contents(&self) -> Option<SharedContents> {
        None
    }

    /// The file's readiness, registering on its queues through `table` (`file_operations.poll`).
    /// Files are always ready (`DEFAULT_POLLMASK`); a driver whose files report changes some
    /// other way, as kernfs reports `POLLPRI`, says so here.
    fn poll(&self, _table: Option<&mut PollTable<'_>>) -> PollMask {
        PollMask::DEFAULT
    }

    /// Reads entries starting at `cookie` (0 is the start) into `sink` until it is full or the
    /// directory ends. Never produces `.` or `..`, which the VFS adds. Cookies are opaque, at
    /// most `MAX_DIR_COOKIE`, and resume correctly while entries are added and removed: an
    /// entry present for the whole read is produced exactly once, as tmpfs guarantees.
    async fn readdir(&self, _cookie: u64, _sink: &mut dyn DirSink) -> FsResult<()> {
        Err(Errno::ENOTDIR)
    }
}

/// Contents a driver shares rather than copies (see [`Handle::shared_contents`]).
pub type SharedContents = Arc<dyn AsRef<[u8]> + Send + Sync>;

/// Largest cookie a driver may hand out: `telldir` returns a `long`, 32 bits on wasm32, and the
/// VFS reserves 0..=2 for its own positions.
pub const MAX_DIR_COOKIE: u64 = i32::MAX as u64 - 3;
