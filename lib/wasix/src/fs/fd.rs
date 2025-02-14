use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    path::PathBuf,
    pin::Pin,
    sync::{atomic::AtomicU64, Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
    task::Context,
};

use futures::Future;
use serde_derive::{Deserialize, Serialize};
use std::sync::Mutex as StdMutex;
use tokio::sync::{watch, Mutex as AsyncMutex};
use virtual_fs::{Pipe, PipeRx, PipeTx, VirtualFile};
use wasmer_wasix_types::wasi::{EpollType, Fd as WasiFd, Fdflags, Fdflagsext, Filestat, Rights};

use crate::{net::socket::InodeSocket, syscalls::EpollJoinWaker};

use super::{
    InodeGuard, InodeValFilePollGuard, InodeValFilePollGuardJoin, InodeValFilePollGuardMode,
    InodeWeakGuard, NotificationInner,
};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Fd {
    #[cfg_attr(feature = "enable-serde", serde(flatten))]
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
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
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
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct InodeVal {
    pub stat: RwLock<Filestat>,
    pub is_preopened: bool,
    pub name: RwLock<Cow<'static, str>>,
    pub kind: RwLock<Kind>,
}

impl InodeVal {
    pub fn read(&self) -> RwLockReadGuard<Kind> {
        self.kind.read().unwrap()
    }

    pub fn write(&self) -> RwLockWriteGuard<Kind> {
        self.kind.write().unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpollFd {
    /// The events we are polling on
    pub events: EpollType,
    /// Pointer to the user data
    pub ptr: u64,
    /// File descriptor we are polling on
    pub fd: WasiFd,
    /// Associated user data
    pub data1: u32,
    /// Associated user data
    pub data2: u64,
}

/// Represents all the EpollInterests that have occurred
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct EpollInterest {
    /// Using a hash set prevents the same interest from
    /// being triggered more than once
    pub interest: HashSet<(WasiFd, EpollType)>,
}

/// Guard the cleans up the selector registrations
#[derive(Debug)]
pub enum EpollJoinGuard {
    Join {
        join_guard: InodeValFilePollGuardJoin,
        epoll_waker: Arc<EpollJoinWaker>,
    },
    Handler {
        fd_guard: InodeValFilePollGuard,
    },
}
impl Drop for EpollJoinGuard {
    fn drop(&mut self) {
        if let Self::Handler { fd_guard, .. } = self {
            if let InodeValFilePollGuardMode::Socket { inner } = &mut fd_guard.mode {
                let mut inner = inner.protected.write().unwrap();
                inner.remove_handler();
            }
        }
    }
}
impl EpollJoinGuard {
    pub fn is_spent(&self) -> bool {
        match self {
            Self::Join { join_guard, .. } => join_guard.is_spent(),
            Self::Handler { .. } => false,
        }
    }
    pub fn renew(&mut self) {
        if let Self::Join {
            join_guard,
            epoll_waker,
        } = self
        {
            let fd = join_guard.fd();
            join_guard.reset();

            let waker = epoll_waker.as_waker();
            let mut cx = Context::from_waker(&waker);
            if Pin::new(join_guard).poll(&mut cx).is_ready() {
                tracing::trace!(fd, "join renew already woken");
                waker.wake();
            } else {
                tracing::trace!(fd, "join waker reinstalled");
            }
        }
    }
}

pub type EpollSubscriptions = HashMap<WasiFd, (EpollFd, Vec<EpollJoinGuard>)>;

/// The core of the filesystem abstraction.  Includes directories,
/// files, and symlinks.
#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum Kind {
    File {
        /// The open file, if it's open
        #[cfg_attr(feature = "enable-serde", serde(skip))]
        handle: Option<Arc<RwLock<Box<dyn VirtualFile + Send + Sync + 'static>>>>,
        /// The path on the host system where the file is located
        /// This is deprecated and will be removed soon
        path: PathBuf,
        /// Marks the file as a special file that only one `fd` can exist for
        /// This is useful when dealing with host-provided special files that
        /// should be looked up by path
        /// TOOD: clarify here?
        fd: Option<u32>,
    },
    #[cfg_attr(feature = "enable-serde", serde(skip))]
    Socket {
        /// Represents a networking socket
        socket: InodeSocket,
    },
    #[cfg_attr(feature = "enable-serde", serde(skip))]
    PipeTx {
        tx: PipeTx,
    },
    #[cfg_attr(feature = "enable-serde", serde(skip))]
    PipeRx {
        rx: PipeRx,
    },
    #[cfg_attr(feature = "enable-serde", serde(skip))]
    DuplexPipe {
        pipe: Pipe,
    },
    Epoll {
        // List of events we are polling on
        subscriptions: Arc<StdMutex<EpollSubscriptions>>,
        // Notification pipeline for sending events
        tx: Arc<watch::Sender<EpollInterest>>,
        // Notification pipeline for events that need to be
        // checked on the next wait
        rx: Arc<AsyncMutex<watch::Receiver<EpollInterest>>>,
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
    /// The first two fields are data _about_ the symlink
    /// the last field is the data _inside_ the symlink
    ///
    /// `base_po_dir` should never be the root because:
    /// - Right now symlinks are not allowed in the immutable root
    /// - There is always a closer pre-opened dir to the symlink file (by definition of the root being a collection of preopened dirs)
    Symlink {
        /// The preopened dir that this symlink file is relative to (via `path_to_symlink`)
        base_po_dir: WasiFd,
        /// The path to the symlink from the `base_po_dir`
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
