use std::{
    borrow::Cow,
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard, atomic::AtomicU64},
};

use tokio::sync::Mutex;

use virtual_fs::{Pipe, PipeRx, PipeTx, VirtualFile};
use wasmer_wasix_types::wasi::{Fdflags, Fdflagsext, Filestat, Rights};

use crate::net::socket::InodeSocket;
use crate::os::epoll::EpollState;

use super::{InodeGuard, InodeWeakGuard, NotificationInner};

/// Shared handle to an open [`VirtualFile`].
pub(crate) type VirtualFileLock = Arc<Mutex<Box<dyn VirtualFile + Send + Sync + 'static>>>;

#[derive(Debug, Clone)]
pub struct Fd {
    pub inner: FdInner,

    /// Flags that determine how the [`Fd`] can be used.
    ///
    /// Used when reopening a [`VirtualFile`] during deserialization.
    pub open_flags: u16,
    pub inode: InodeGuard,
    pub is_stdio: bool,
}

// This struct contains the bits of Fd that are safe to mutate, so that
// FdList::get_mut can safely return mutable references.
#[derive(Debug, Clone)]
pub struct FdInner {
    pub rights: Rights,
    pub rights_inheriting: Rights,
    pub flags: Fdflags,         // This is file table related flags, not fd flags
    pub offset: Arc<AtomicU64>, // This also belongs in the file table
    pub fd_flags: Fdflagsext,   // This is the actual FD flags that belongs here
}

impl Fd {
    /// This [`Fd`] can be used with read system calls.
    pub const READ: u16 = 1;
    /// This [`Fd`] can be used with write system calls.
    pub const WRITE: u16 = 2;
    /// This [`Fd`] can append in write system calls. Note that the append
    /// permission implies the write permission.
    pub const APPEND: u16 = 4;
    /// This [`Fd`] will delete everything before writing. Note that truncate
    /// permissions require the write permission.
    ///
    /// This permission is currently unused when deserializing.
    pub const TRUNCATE: u16 = 8;
    /// This [`Fd`] may create a file before writing to it. Note that create
    /// permissions require write permissions.
    ///
    /// This permission is currently unused when deserializing.
    pub const CREATE: u16 = 16;
}

/// A file that Wasi knows about that may or may not be open
#[derive(Debug)]
pub struct InodeVal {
    pub stat: RwLock<Filestat>,
    pub is_preopened: bool,
    pub name: RwLock<Cow<'static, str>>,
    pub kind: RwLock<Kind>,
}

impl InodeVal {
    pub fn read(&self) -> RwLockReadGuard<'_, Kind> {
        self.kind.read().unwrap()
    }

    pub fn write(&self) -> RwLockWriteGuard<'_, Kind> {
        self.kind.write().unwrap()
    }
}

/// The core of the filesystem abstraction.  Includes directories,
/// files, and symlinks.
#[derive(Debug)]
pub enum Kind {
    File {
        /// The open file, if it's open
        handle: Option<VirtualFileLock>,
        /// The path on the host system where the file is located
        /// This is deprecated and will be removed soon
        path: PathBuf,
        /// Marks the file as a special file that only one `fd` can exist for
        /// This is useful when dealing with host-provided special files that
        /// should be looked up by path
        /// TODO: clarify here?
        fd: Option<u32>,
    },
    Socket {
        /// Represents a networking socket
        socket: InodeSocket,
    },
    PipeTx {
        tx: PipeTx,
    },
    PipeRx {
        rx: PipeRx,
    },
    DuplexPipe {
        pipe: Pipe,
    },
    Epoll {
        state: Arc<EpollState>,
    },
    Dir {
        /// Parent directory
        parent: InodeWeakGuard,
        /// The path on the host system where the directory is located
        // TODO: wrap it like VirtualFile
        path: PathBuf,
        /// The entries of a directory are lazily filled.
        entries: HashMap<String, InodeGuard>,
    },
    /// The same as Dir but without the irrelevant bits
    /// The root is immutable after creation; generally the Kind::Root
    /// branch of whatever code you're writing will be a simpler version of
    /// your Kind::Dir logic
    Root {
        entries: HashMap<String, InodeGuard>,
    },
    /// The first two fields are data _about_ the symlink; the last field is
    /// the data _inside_ the symlink.
    Symlink {
        /// Whether the link came from the backing filesystem or from a WASI
        /// `path_symlink` call. Backing links are resolved within their mount;
        /// virtual links are resolved from the WASIX virtual root.
        symlink_kind: SymlinkKind,
        /// Full path to the symlink from the WASIX virtual root, with no
        /// leading slash.
        path_to_symlink: PathBuf,
        /// the value of the symlink as a relative path
        relative_path: PathBuf,
    },
    Buffer {
        buffer: Vec<u8>,
    },
    EventNotifications {
        inner: Arc<NotificationInner>,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum SymlinkKind {
    Backing,
    Virtual,
}
