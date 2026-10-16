//! Read-only filesystem driver over a webc v3 package volume, after Linux's squashfs and erofs.
//!
//! Only the v3 reader is used. A directory's entries are parsed the first time it is looked up
//! or read, into a table shared by every node for it; lookups then binary-search the table, which
//! is in name order because v3 writes it from a `BTreeMap`. File data is a slice of the
//! container's buffer (an mmap or `Bytes`), so nothing is copied until a read fills the caller's
//! buffer.
//!
//! What the image does not record is fixed: every object belongs to root (uid and gid 0) unless
//! the embedder picks another owner, files and directories are `0755`, symlinks `0777`. Files are executable because v3 stores no mode
//! bits and packages carry programs and scripts; nothing can be written anyway. All timestamps
//! are the epoch, since the v3 reader does not expose the stored modification time. Inode
//! numbers are handed out from a per-volume counter when a directory is first parsed (the root
//! is 1, its entries take the next consecutive numbers), so every object has exactly one
//! number for the mount's lifetime and no two objects share one.

use std::{
    any::Any,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use shared_buffer::OwnedBuffer;
use wasixv2_abi::Errno;
use wasixv2_vfs::{
    AllocMode, Attr, Device, DirEntry, DirSink, FileType, Filesystem, FsInfo, FsResult, Handle,
    MAX_DIR_COOKIE, NAME_MAX, NewNode, Node, OpenOptions, RenameMode, SetAttr, StatFs, Timestamp,
    XattrSet,
};
use webc::v3::read::{
    DirEntry as WebcEntry, Directory, OwnedReader, OwnedReaderError, VolumeHeaderError,
    VolumeSection,
};

#[cfg(test)]
mod tests;

/// `f_type` of `statfs`: "webc" in ASCII. No Linux filesystem uses it.
pub const WEBC_MAGIC: u32 = 0x7765_6263;
const BLOCK: u64 = 4096;
const ROOT_INO: u64 = 1;
const FILE_MODE: u32 = 0o755;
const DIR_MODE: u32 = 0o755;
const SYMLINK_MODE: u32 = 0o777;

/// Why a volume cannot be mounted.
#[derive(Debug, thiserror::Error)]
pub enum PackageFsError {
    #[error(transparent)]
    Volume(#[from] OwnedReaderError),
    #[error("unable to read the volume's root directory")]
    Header(#[from] VolumeHeaderError),
}

/// One volume of a webc v3 package, mounted read-only.
pub struct PackageFs {
    root: Arc<DirData>,
    vol: Arc<Volume>,
}

/// Keeps the volume's buffers alive for the `'static` borrows in the directory tables. Everything
/// holding such a borrow also holds an `Arc` of this, declared after the borrow so it drops last.
struct Volume {
    // Never read: it owns the header bytes that `Directory<'static>` and names point into.
    _section: VolumeSection,
    size: u64,
    /// The uid and gid every object reports.
    owner: (u32, u32),
    /// The next inode number to hand out.
    next_ino: AtomicU64,
}

impl PackageFs {
    /// Mounts the volume `name` of `reader` (for example `"atom"`, or a name from the
    /// manifest's `fs` annotation). Shares the reader's buffer.
    pub fn new(reader: &OwnedReader, name: &str) -> Result<Self, PackageFsError> {
        Self::with_owner(reader, name, 0, 0)
    }

    /// Like [`PackageFs::new`], with every object owned by `uid` and `gid` instead of root, so
    /// that an overlay copying them up gives that user files it can change.
    pub fn with_owner(
        reader: &OwnedReader,
        name: &str,
        uid: u32,
        gid: u32,
    ) -> Result<Self, PackageFsError> {
        let section = reader.get_volume(name)?;
        let size = reader.index().volumes.get(name).map_or(0, |e| e.span.len) as u64;
        let root = section.root()?;
        // SAFETY: `root` borrows the header bytes inside `section`'s `OwnedBuffer`, which are
        // reference-counted heap or mmap memory that does not move with the `VolumeSection`.
        // `section` lives in the `Volume` every node holds an `Arc` of, so the bytes outlive
        // every `Directory<'static>` and `&'static str` derived from them, none of which escape
        // this crate.
        let root = unsafe { std::mem::transmute::<Directory<'_>, Directory<'static>>(root) };
        let vol = Arc::new(Volume {
            _section: section,
            size,
            owner: (uid, gid),
            next_ino: AtomicU64::new(ROOT_INO + 1),
        });
        let root = Arc::new(DirData::new(root, vol.clone()));
        Ok(Self { root, vol })
    }
}

#[async_trait(?Send)]
impl Filesystem for PackageFs {
    fn info(&self) -> FsInfo {
        FsInfo {
            name: "webc",
            magic: WEBC_MAGIC,
            name_max: NAME_MAX,
            read_only: true,
            dev: None,
            has_btime: false,
            stable: true,
        }
    }

    fn root(&self) -> Arc<dyn Node> {
        Arc::new(PackageNode {
            kind: Kind::Dir(self.root.clone()),
            ino: ROOT_INO,
            vol: self.vol.clone(),
        })
    }

    async fn statfs(&self) -> FsResult<StatFs> {
        // Like squashfs: the image's size, nothing free. The object count is unknown without
        // walking the whole tree.
        Ok(StatFs {
            block_size: BLOCK as u32,
            blocks: self.vol.size.div_ceil(BLOCK),
            blocks_free: 0,
            blocks_avail: 0,
            files: 0,
            files_free: 0,
            name_max: NAME_MAX as u32,
            fragment_size: BLOCK as u32,
        })
    }
}

/// A directory and, once parsed, its entries.
struct DirData {
    dir: Directory<'static>,
    children: OnceLock<Result<Children, Errno>>,
    vol: Arc<Volume>,
}

struct Children {
    /// In byte order of the names.
    entries: Box<[Child]>,
    subdirs: u32,
}

struct Child {
    name: &'static str,
    ino: u64,
    kind: Kind,
}

#[derive(Clone)]
enum Kind {
    Dir(Arc<DirData>),
    File(OwnedBuffer),
    Symlink(&'static str),
}

impl Kind {
    fn file_type(&self) -> FileType {
        match self {
            Kind::Dir(_) => FileType::Directory,
            Kind::File(_) => FileType::Regular,
            Kind::Symlink(_) => FileType::Symlink,
        }
    }
}

impl DirData {
    fn new(dir: Directory<'static>, vol: Arc<Volume>) -> Self {
        Self {
            dir,
            children: OnceLock::new(),
            vol,
        }
    }

    fn children(&self) -> FsResult<&Children> {
        self.children
            .get_or_init(|| self.parse())
            .as_ref()
            .map_err(|e| *e)
    }

    fn parse(&self) -> FsResult<Children> {
        let mut entries = Vec::new();
        let mut subdirs = 0;
        let parsed = self
            .dir
            .entries()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| Errno::EIO)?;
        // `children` runs this once per directory, so each entry gets its number exactly once.
        let first = self
            .vol
            .next_ino
            .fetch_add(parsed.len() as u64, Ordering::Relaxed);
        for (ino, (name, _hash, entry)) in (first..).zip(parsed) {
            let kind = match entry {
                WebcEntry::Dir(dir) => {
                    subdirs += 1;
                    Kind::Dir(Arc::new(DirData::new(dir, self.vol.clone())))
                }
                WebcEntry::File(file) => Kind::File(file.bytes().clone()),
                WebcEntry::Symlink(link) => Kind::Symlink(link.target()),
            };
            entries.push(Child { name, ino, kind });
        }
        if entries.len() as u64 > MAX_DIR_COOKIE {
            return Err(Errno::EOVERFLOW);
        }
        // The writer emits names in order; a foreign image that does not still gets lookups.
        if !entries.is_sorted_by(|a, b| a.name <= b.name) {
            entries.sort_unstable_by(|a, b| a.name.cmp(b.name));
        }
        Ok(Children {
            entries: entries.into_boxed_slice(),
            subdirs,
        })
    }
}

/// A file, directory or symlink of the volume.
pub struct PackageNode {
    kind: Kind,
    ino: u64,
    vol: Arc<Volume>,
}

impl PackageNode {
    fn dir(&self) -> FsResult<&Arc<DirData>> {
        match &self.kind {
            Kind::Dir(dir) => Ok(dir),
            _ => Err(Errno::ENOTDIR),
        }
    }
}

#[async_trait(?Send)]
impl Node for PackageNode {
    fn ino(&self) -> u64 {
        self.ino
    }

    fn file_type(&self) -> FileType {
        self.kind.file_type()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn getattr(&self) -> FsResult<Attr> {
        let (mode, nlink, size) = match &self.kind {
            Kind::Dir(dir) => (DIR_MODE, 2 + dir.children()?.subdirs, BLOCK),
            Kind::File(data) => (FILE_MODE, 1, data.len() as u64),
            Kind::Symlink(target) => (SYMLINK_MODE, 1, target.len() as u64),
        };
        let epoch = Timestamp::default();
        Ok(Attr {
            ino: self.ino,
            file_type: self.kind.file_type(),
            mode,
            nlink,
            uid: self.vol.owner.0,
            gid: self.vol.owner.1,
            rdev: Device::default(),
            size,
            blocks: size.div_ceil(512),
            blksize: BLOCK as u32,
            atime: epoch,
            mtime: epoch,
            ctime: epoch,
            btime: None,
            dev: None,
        })
    }

    async fn setattr(&self, _attr: &SetAttr) -> FsResult<Attr> {
        Err(Errno::EROFS)
    }

    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        let children = self.dir()?.children()?;
        let index = children
            .entries
            .binary_search_by(|c| c.name.as_bytes().cmp(name))
            .map_err(|_| Errno::ENOENT)?;
        let child = &children.entries[index];
        Ok(Arc::new(PackageNode {
            kind: child.kind.clone(),
            ino: child.ino,
            vol: self.vol.clone(),
        }))
    }

    async fn create(&self, _name: &[u8], _new: &NewNode<'_>) -> FsResult<Arc<dyn Node>> {
        self.dir()?;
        Err(Errno::EROFS)
    }

    async fn create_unnamed(
        &self,
        _new: &NewNode<'_>,
        _options: OpenOptions,
    ) -> FsResult<(Arc<dyn Node>, Box<dyn Handle>)> {
        Err(Errno::EROFS)
    }

    async fn link(&self, _name: &[u8], _target: &Arc<dyn Node>) -> FsResult<()> {
        self.dir()?;
        Err(Errno::EROFS)
    }

    async fn unlink(&self, _name: &[u8]) -> FsResult<()> {
        self.dir()?;
        Err(Errno::EROFS)
    }

    async fn rmdir(&self, _name: &[u8]) -> FsResult<()> {
        self.dir()?;
        Err(Errno::EROFS)
    }

    async fn rename(
        &self,
        _name: &[u8],
        _new_dir: &Arc<dyn Node>,
        _new_name: &[u8],
        _mode: RenameMode,
    ) -> FsResult<()> {
        self.dir()?;
        Err(Errno::EROFS)
    }

    async fn readlink(&self) -> FsResult<Vec<u8>> {
        match &self.kind {
            Kind::Symlink(target) => Ok(target.as_bytes().to_vec()),
            _ => Err(Errno::EINVAL),
        }
    }

    async fn open(&self, options: OpenOptions) -> FsResult<Box<dyn Handle>> {
        if options.write {
            return Err(Errno::EROFS);
        }
        Ok(match &self.kind {
            Kind::Dir(dir) => Box::new(DirHandle(dir.clone())),
            Kind::File(data) => Box::new(FileHandle(data.clone())),
            Kind::Symlink(_) => return Err(Errno::ELOOP),
        })
    }

    async fn getxattr(&self, _name: &[u8]) -> FsResult<Vec<u8>> {
        Err(Errno::ENODATA)
    }

    async fn setxattr(&self, _name: &[u8], _value: &[u8], _how: XattrSet) -> FsResult<()> {
        Err(Errno::EROFS)
    }

    async fn listxattr(&self) -> FsResult<Vec<Vec<u8>>> {
        Ok(Vec::new())
    }

    async fn removexattr(&self, _name: &[u8]) -> FsResult<()> {
        Err(Errno::EROFS)
    }
}

/// An open regular file: a slice of the container's buffer.
struct FileHandle(OwnedBuffer);

#[async_trait(?Send)]
impl Handle for FileHandle {
    async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let data = self.0.as_slice();
        let Some(rest) = usize::try_from(offset).ok().and_then(|o| data.get(o..)) else {
            return Ok(0);
        };
        let n = rest.len().min(buf.len());
        buf[..n].copy_from_slice(&rest[..n]);
        Ok(n)
    }

    async fn write_at(&self, _data: &[u8], _offset: u64) -> FsResult<usize> {
        Err(Errno::EROFS)
    }

    async fn append(&self, _data: &[u8]) -> FsResult<(u64, usize)> {
        Err(Errno::EROFS)
    }

    async fn allocate(&self, _mode: AllocMode, _offset: u64, _len: u64) -> FsResult<()> {
        Err(Errno::EROFS)
    }

    async fn set_size(&self, _size: u64) -> FsResult<()> {
        Err(Errno::EROFS)
    }
}

/// An open directory. Cookies are positions in the name-ordered table, which never changes.
struct DirHandle(Arc<DirData>);

#[async_trait(?Send)]
impl Handle for DirHandle {
    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()> {
        let entries = &self.0.children()?.entries;
        let start = usize::try_from(cookie).unwrap_or(usize::MAX);
        for (i, child) in entries.iter().enumerate().skip(start) {
            let entry = DirEntry {
                name: child.name.as_bytes(),
                ino: child.ino,
                file_type: child.kind.file_type(),
                next: i as u64 + 1,
            };
            if !sink.push(entry) {
                break;
            }
        }
        Ok(())
    }
}
