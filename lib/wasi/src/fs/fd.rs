use generational_arena::Index as Inode;
#[cfg(feature = "enable-serde")]
use serde_derive::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use wasmer_vfs::{VirtualFile, WasiPipe};
use wasmer_wasi_types::wasi::{Fd as WasiFd, Fdflags, Filestat, Rights};

use crate::net::socket::InodeSocket;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Fd {
    /// The reference count is only increased when the FD is
    /// duplicates - fd_close will not kill the inode until this reaches zero
    pub ref_cnt: Arc<AtomicU32>,
    pub rights: Rights,
    pub rights_inheriting: Rights,
    pub flags: Fdflags,
    pub offset: Arc<AtomicU64>,
    /// Flags that determine how the [`Fd`] can be used.
    ///
    /// Used when reopening a [`VirtualFile`] during [`WasiState`] deserialization.
    pub open_flags: u16,
    pub inode: Inode,
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
    /// This permission is currently unused when deserializing [`WasiState`].
    pub const TRUNCATE: u16 = 8;
    /// This [`Fd`] may create a file before writing to it. Note that create
    /// permissions require write permissions.
    ///
    /// This permission is currently unused when deserializing [`WasiState`].
    pub const CREATE: u16 = 16;
}

/// A file that Wasi knows about that may or may not be open
#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct InodeVal {
    pub stat: RwLock<Filestat>,
    pub is_preopened: bool,
    pub name: Cow<'static, str>,
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
    Pipe {
        /// Reference to the pipe
        pipe: WasiPipe,
    },
    Dir {
        /// Parent directory
        parent: Option<Inode>,
        /// The path on the host system where the directory is located
        // TODO: wrap it like VirtualFile
        path: PathBuf,
        /// The entries of a directory are lazily filled.
        entries: HashMap<String, Inode>,
    },
    /// The same as Dir but without the irrelevant bits
    /// The root is immutable after creation; generally the Kind::Root
    /// branch of whatever code you're writing will be a simpler version of
    /// your Kind::Dir logic
    Root {
        entries: HashMap<String, Inode>,
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
        /// Used for event notifications by the user application or operating system
        /// (positive number means there are events waiting to be processed)
        counter: Arc<AtomicU64>,
        /// Flag that indicates if this is operating
        is_semaphore: bool,
        /// Receiver that wakes sleeping threads
        #[cfg_attr(feature = "enable-serde", serde(skip))]
        wakers: Arc<Mutex<VecDeque<tokio::sync::mpsc::UnboundedSender<()>>>>,
        /// Immediate waker
        immediate: Arc<AtomicBool>,
    },
}
