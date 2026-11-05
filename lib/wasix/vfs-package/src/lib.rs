//! Read-only filesystem driver over a webc v3 package volume, after Linux's squashfs and erofs,
//! or over files given as byte buffers, such as a package's commands.
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
//!
//! The image is untrusted: mounting walks its whole directory tree once and refuses an image
//! whose entries cannot be decoded, whose names are not valid file names, or whose directories
//! are shared or loop, as squashfs and erofs reject corrupt metadata instead of trusting it.

//!
//! [`PackageFs::from_files`] builds the same kind of tree from paths and buffers, with the
//! directories their paths imply; it is how a package tree's commands appear in `/bin`.

use std::{
    any::Any,
    borrow::Cow,
    collections::{BTreeMap, btree_map},
    panic::AssertUnwindSafe,
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
    MAX_DIR_COOKIE, NAME_MAX, NewNode, Node, OpenOptions, PATH_MAX, RenameMode, SetAttr,
    SharedContents, StatFs, Timestamp, XattrSet,
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
/// The fewest header bytes a directory entry takes in a well-formed image: its offset, hash and
/// name length (the name itself is at least a byte more).
const MIN_ENTRY_SIZE: u64 = 8 + 32 + 8;

/// Why a volume cannot be mounted.
#[derive(Debug, thiserror::Error)]
pub enum PackageFsError {
    #[error(transparent)]
    Volume(#[from] OwnedReaderError),
    #[error("unable to read the volume's root directory")]
    Header(#[from] VolumeHeaderError),
    #[error("the volume is malformed: {0}")]
    Malformed(&'static str),
    #[error("\"{0}\" is not a relative path of names that are valid file names")]
    InvalidPath(String),
    #[error("\"{0}\" is given twice, or both as a file and as a directory")]
    Conflict(String),
    #[error("a directory has more entries than can be listed")]
    TooManyEntries,
    #[error("a path of {0} bytes is longer than PATH_MAX allows")]
    PathTooLong(usize),
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
    // `None` for a tree built from buffers, which own their data.
    _section: Option<VolumeSection>,
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
        // webc adds the span's start and length unchecked.
        let span = reader.index().volumes.get(name).map(|entry| entry.span);
        if span.is_some_and(|span| span.start.checked_add(span.len).is_none()) {
            return Err(PackageFsError::Malformed(
                "the volume lies outside the file",
            ));
        }
        let size = span.map_or(0, |span| span.len) as u64;
        let section = reader.get_volume(name)?;
        let root = section.root()?;
        // SAFETY: `root` borrows the header bytes inside `section`'s `OwnedBuffer`, which are
        // reference-counted heap or mmap memory that does not move with the `VolumeSection`.
        // `section` lives in the `Volume` every node holds an `Arc` of, so the bytes outlive
        // every `Directory<'static>` and `&'static str` derived from them, none of which escape
        // this crate.
        let root = unsafe { std::mem::transmute::<Directory<'_>, Directory<'static>>(root) };
        validate(&root, size)?;
        let vol = Arc::new(Volume {
            _section: Some(section),
            size,
            owner: (uid, gid),
            next_ino: AtomicU64::new(ROOT_INO + 1),
        });
        let root = Arc::new(DirData::new(root, vol.clone()));
        Ok(Self { root, vol })
    }

    /// A read-only tree of the regular files `files`, each a relative path (such as
    /// `bin/ls`) and its contents, with the directories the paths imply. Every object is
    /// owned by `uid` and `gid` and has the modes a volume's do. Shares the buffers. Fails
    /// when a path has a component that is not a valid file name (empty, `.`, `..`, with a
    /// NUL, or longer than `NAME_MAX`), when a path is longer than a path argument may be
    /// (`PATH_MAX` with its NUL), when one path is given twice or both as a file and as a
    /// directory, or when a directory would have more entries than `getdents` can number.
    pub fn from_files(
        files: impl IntoIterator<Item = (String, OwnedBuffer)>,
        uid: u32,
        gid: u32,
    ) -> Result<Self, PackageFsError> {
        let mut tree = Tree::default();
        let mut size = 0;
        for (path, data) in files {
            size += data.len() as u64;
            tree.insert(&path, data)?;
        }
        let vol = Arc::new(Volume {
            _section: None,
            size,
            owner: (uid, gid),
            next_ino: AtomicU64::new(ROOT_INO + 1),
        });
        let root = tree.build(&vol)?;
        Ok(Self { root, vol })
    }
}

/// The directories of [`PackageFs::from_files`] while they are collected.
#[derive(Default)]
struct Tree(BTreeMap<String, TreeEntry>);

enum TreeEntry {
    Dir(Tree),
    File(OwnedBuffer),
}

impl Tree {
    fn insert(&mut self, path: &str, data: OwnedBuffer) -> Result<(), PackageFsError> {
        // The bound on the length bounds the depth, which building and dropping the tree
        // recurse through.
        if path.len() >= PATH_MAX {
            return Err(PackageFsError::PathTooLong(path.len()));
        }
        let invalid = || PackageFsError::InvalidPath(path.to_string());
        let conflict = || PackageFsError::Conflict(path.to_string());
        let names: Vec<&str> = path.split('/').collect();
        if !names.iter().all(|name| valid_name(name)) {
            return Err(invalid());
        }
        let (file, dirs) = names.split_last().ok_or_else(invalid)?;
        let mut dir = self;
        for name in dirs {
            let entry = dir
                .0
                .entry(name.to_string())
                .or_insert_with(|| TreeEntry::Dir(Tree::default()));
            dir = match entry {
                TreeEntry::Dir(tree) => tree,
                TreeEntry::File(_) => return Err(conflict()),
            };
        }
        match dir.0.entry(file.to_string()) {
            btree_map::Entry::Vacant(entry) => {
                entry.insert(TreeEntry::File(data));
                Ok(())
            }
            btree_map::Entry::Occupied(_) => Err(conflict()),
        }
    }

    /// The directory's table, its entries numbered consecutively before those of its
    /// subdirectories, as a parsed volume numbers them. Iterative, so a deep tree cannot
    /// exhaust the stack: directories are numbered in depth-first order, then built from the
    /// deepest up.
    fn build(self, vol: &Arc<Volume>) -> Result<Arc<DirData>, PackageFsError> {
        enum Slot {
            File(OwnedBuffer),
            /// The index of the subdirectory's table in `tables`.
            Dir(usize),
        }
        let mut tables: Vec<Vec<(String, u64, Slot)>> = Vec::new();
        // A directory to number, and where its parent's table names it.
        let mut pending = vec![(self, None::<(usize, usize)>)];
        while let Some((tree, parent)) = pending.pop() {
            if tree.0.len() as u64 > MAX_DIR_COOKIE {
                return Err(PackageFsError::TooManyEntries);
            }
            let table = tables.len();
            if let Some((at, entry)) = parent {
                tables[at][entry].2 = Slot::Dir(table);
            }
            let first = vol
                .next_ino
                .fetch_add(tree.0.len() as u64, Ordering::Relaxed);
            let mut entries = Vec::with_capacity(tree.0.len());
            let mut subdirs = Vec::new();
            for (i, (ino, (name, entry))) in (first..).zip(tree.0).enumerate() {
                match entry {
                    TreeEntry::File(data) => entries.push((name, ino, Slot::File(data))),
                    TreeEntry::Dir(sub) => {
                        entries.push((name, ino, Slot::Dir(usize::MAX)));
                        subdirs.push((sub, Some((table, i))));
                    }
                }
            }
            tables.push(entries);
            // The first subdirectory is numbered next, as a recursive walk would.
            pending.extend(subdirs.into_iter().rev());
        }
        // A subdirectory's table comes after its parent's, so building from the end finds
        // every subdirectory built.
        let mut built: Vec<Option<Arc<DirData>>> = (0..tables.len()).map(|_| None).collect();
        for (at, entries) in tables.into_iter().enumerate().rev() {
            let mut subdirs = 0;
            let entries = entries
                .into_iter()
                .map(|(name, ino, slot)| {
                    let kind = match slot {
                        Slot::File(data) => Kind::File(data),
                        Slot::Dir(table) => {
                            subdirs += 1;
                            Kind::Dir(built[table].take().expect("built before its parent"))
                        }
                    };
                    Child {
                        name: Cow::Owned(name),
                        ino,
                        kind,
                    }
                })
                .collect::<Vec<_>>();
            let children = Children {
                entries: entries.into_boxed_slice(),
                subdirs,
            };
            built[at] = Some(Arc::new(DirData {
                source: None,
                children: OnceLock::from(Ok(children)),
                vol: vol.clone(),
            }));
        }
        Ok(built[0].take().expect("the root is built"))
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
    /// The volume's directory, `None` for one built with its entries.
    source: Option<Directory<'static>>,
    children: OnceLock<Result<Children, Errno>>,
    vol: Arc<Volume>,
}

struct Children {
    /// In byte order of the names.
    entries: Box<[Child]>,
    subdirs: u32,
}

struct Child {
    /// Borrowed from the volume header, or owned for a built tree.
    name: Cow<'static, str>,
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
            source: Some(dir),
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
        // A built directory has its table from the start, so only volume directories get here;
        // mounting decoded every one of them already, so this cannot fail on the same bytes.
        let dir = self.source.as_ref().ok_or(Errno::EIO)?;
        let parsed = decode(dir).map_err(|_| Errno::EIO)?;
        // `children` runs this once per directory, so each entry gets its number exactly once.
        let first = self
            .vol
            .next_ino
            .fetch_add(parsed.len() as u64, Ordering::Relaxed);
        for (ino, (name, entry)) in (first..).zip(parsed) {
            let kind = match entry {
                WebcEntry::Dir(dir) => {
                    subdirs += 1;
                    Kind::Dir(Arc::new(DirData::new(dir, self.vol.clone())))
                }
                WebcEntry::File(file) => Kind::File(file.bytes().clone()),
                WebcEntry::Symlink(link) => Kind::Symlink(link.target()),
            };
            entries.push(Child {
                name: Cow::Borrowed(name),
                ino,
                kind,
            });
        }
        Ok(Children {
            entries: entries.into_boxed_slice(),
            subdirs,
        })
    }
}

type Entries = Vec<(&'static str, WebcEntry<'static>)>;

/// The entries of `dir` in name order, checked: each decodes, and each name is a single valid
/// file name that occurs once.
fn decode(dir: &Directory<'static>) -> Result<Entries, &'static str> {
    // webc 12 panics on some malformed entries instead of failing: a file whose data starts
    // past its end slices the data section backwards, and a header number above `usize::MAX`
    // on a 32-bit host. Contain that as a malformed image; it only works where panics unwind,
    // so webc itself must check these.
    let decoded = std::panic::catch_unwind(AssertUnwindSafe(|| {
        dir.entries()
            .map(|entry| entry.map(|(name, _hash, entry)| (name, entry)))
            .collect::<Result<Entries, _>>()
    }));
    let mut entries = decoded
        .map_err(|_| "an entry lies outside the volume")?
        .map_err(|_| "an entry cannot be decoded")?;
    if entries.iter().any(|(name, _)| !valid_name(name)) {
        return Err("an entry's name is not a file name");
    }
    if entries.len() as u64 > MAX_DIR_COOKIE {
        return Err("a directory has too many entries");
    }
    // The writer emits names in order; a foreign image that does not still gets lookups.
    if !entries.is_sorted_by(|a, b| a.0 < b.0) {
        entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
        if entries.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err("a directory has a name twice");
        }
    }
    Ok(entries)
}

/// A name a directory entry may have (what `getname` and `lookup` accept).
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_MAX
        && name != "."
        && name != ".."
        && !name.bytes().any(|b| b == b'/' || b == 0)
}

/// Decodes every directory under `root` once. A tree has at most one entry per
/// `MIN_ENTRY_SIZE` bytes of a `volume_size`-byte volume; more means directories are shared or
/// loop, which would make the tree unbounded.
fn validate(root: &Directory<'static>, volume_size: u64) -> Result<(), PackageFsError> {
    let mut budget = volume_size / MIN_ENTRY_SIZE;
    let mut pending = vec![root.clone()];
    while let Some(dir) = pending.pop() {
        let entries = decode(&dir).map_err(PackageFsError::Malformed)?;
        budget = budget
            .checked_sub(entries.len() as u64)
            .ok_or(PackageFsError::Malformed("directories are shared or loop"))?;
        pending.extend(
            entries
                .into_iter()
                .filter_map(|(_, entry)| entry.into_dir()),
        );
    }
    Ok(())
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

    /// The buffer itself: a volume never changes.
    async fn shared_contents(&self) -> Option<SharedContents> {
        Some(Arc::new(self.0.clone()))
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
