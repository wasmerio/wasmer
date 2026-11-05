//! Owned records for a browser directory provider. These are host records, not a guest ABI.

use async_trait::async_trait;
use wasixv2_vfs::{
    AllocMode, Attr, Device, FileType, FsResult, NewKind, NewNode, OpenOptions, RenameMode,
    SeekHole, SetAttr, StatFs, XattrSet,
};

/// A retained provider object. Tokens must not be reused within a provider session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectId(pub u64);

/// One retained inode reference. Different references may name the same inode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeRef {
    pub id: ObjectId,
    /// Stable identity within this filesystem, including while unlinked but held open.
    pub ino: u64,
    pub generation: u32,
    pub file_type: FileType,
}

/// Resources released without awaiting the provider's event loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    Node(ObjectId),
    Handle(ObjectId),
}

/// Capabilities a provider can implement with the VFS driver's semantics.
///
/// In particular, append and rename are single provider operations; a transport must not
/// implement them with stat-then-write or copy-then-remove. Linux authority:
/// `mm/filemap.c:generic_file_write_iter`, `fs/namei.c:vfs_rename`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub hard_links: bool,
    pub symlinks: bool,
    pub special_files: bool,
    pub xattrs: bool,
    pub atomic_append: bool,
    pub rename_noreplace: bool,
    pub rename_exchange: bool,
    pub unnamed_files: bool,
    pub allocation: bool,
}

/// Owned creation metadata; symlink bodies are bytes, as are directory entry names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Create {
    pub file_type: FileType,
    pub target: Vec<u8>,
    pub device: Device,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
}

impl Create {
    pub(crate) fn from_new(new: &NewNode<'_>) -> Self {
        Self {
            file_type: new.kind.file_type(),
            target: match new.kind {
                NewKind::Symlink(target) => target.to_vec(),
                _ => Vec::new(),
            },
            device: match new.kind {
                NewKind::CharDevice(device) | NewKind::BlockDevice(device) => device,
                _ => Device::default(),
            },
            mode: new.mode,
            uid: new.uid,
            gid: new.gid,
        }
    }

    pub(crate) fn as_new(&self) -> NewNode<'_> {
        NewNode {
            umask: 0,
            kind: match self.file_type {
                FileType::Regular => NewKind::Regular,
                FileType::Directory => NewKind::Directory,
                FileType::Symlink => NewKind::Symlink(&self.target),
                FileType::Fifo => NewKind::Fifo,
                FileType::Socket => NewKind::Socket,
                FileType::CharDevice => NewKind::CharDevice(self.device),
                FileType::BlockDevice => NewKind::BlockDevice(self.device),
            },
            mode: self.mode,
            uid: self.uid,
            gid: self.gid,
            task: 0,
            quota_exempt: false,
        }
    }
}

/// A resumable directory record. No `.` or `..`; VFS supplies those.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: Vec<u8>,
    pub ino: u64,
    pub file_type: FileType,
    pub next: u64,
}

/// One provider operation. Names are single components, never host paths.
#[derive(Clone, Debug)]
pub enum Request {
    Root,
    StatFs,
    SyncFs,
    GetAttr(ObjectId),
    SetAttr(ObjectId, SetAttr),
    Lookup(ObjectId, Vec<u8>),
    Create(ObjectId, Vec<u8>, Create),
    CreateOpen(ObjectId, Vec<u8>, Create, OpenOptions),
    CreateUnnamed(ObjectId, Create, OpenOptions),
    Link(ObjectId, Vec<u8>, ObjectId),
    Unlink(ObjectId, Vec<u8>),
    Rmdir(ObjectId, Vec<u8>),
    Rename(ObjectId, Vec<u8>, ObjectId, Vec<u8>, RenameMode),
    ReadLink(ObjectId),
    Open(ObjectId, OpenOptions),
    GetXattr(ObjectId, Vec<u8>),
    SetXattr(ObjectId, Vec<u8>, Vec<u8>, XattrSet),
    ListXattr(ObjectId),
    RemoveXattr(ObjectId, Vec<u8>),
    Read(ObjectId, u64, usize),
    Write(ObjectId, u64, Vec<u8>),
    Append(ObjectId, Vec<u8>),
    Sync(ObjectId, bool),
    SetSize(ObjectId, u64),
    Allocate(ObjectId, AllocMode, u64, u64),
    SeekHole(ObjectId, u64, u64, SeekHole),
    ReadDir(ObjectId, u64, usize),
}

/// Successful replies. Errors carry Linux errno values in `FsResult`.
#[derive(Clone, Debug)]
pub enum Reply {
    Done,
    Node(NodeRef),
    Open(ObjectId),
    Created(NodeRef, ObjectId),
    Attr(Attr),
    StatFs(StatFs),
    Bytes(Vec<u8>),
    Names(Vec<Vec<u8>>),
    Count(usize),
    Appended(u64, usize),
    Offset(u64),
    Entries(Vec<Entry>),
}

/// Transport to a directory provider's owning event loop.
///
/// Implementations contain shared IDs/queues only, never worker-local JS values. `call`
/// may wait asynchronously; no synchronous mutex may span that wait. B2/B8 supply worker
/// routing. Every returned node/handle is a new lease; `release` enqueues its last-reference
/// cleanup even if its name has gone (`fs/file_table.c:__fput`, `fs/inode.c:iput`). In-flight
/// requests must retain their input objects until completion. Cancellation must release
/// undelivered reply leases, and must not reuse a buffer while a provider can still write it.
/// Session failure completes all waiters with an error; it cannot reconnect old object IDs.
///
/// Providers enforce the Node/Handle contracts, including atomic namespace operations,
/// cookies <= MAX_DIR_COOKIE, and inode identities not reused while referenced. A driver
/// call must not reenter the VFS operation whose async inode lock the caller holds.
#[async_trait(?Send)]
pub trait Provider: Send + Sync {
    async fn call(&self, request: Request) -> FsResult<Reply>;
    fn release(&self, resource: Resource);
    /// Shared invalidation sequence, changed for every externally modified ACL. JS providers
    /// publish it before acknowledging changes so existing VFS inodes discard their ACL cache.
    fn acl_version(&self) -> u64;
}
