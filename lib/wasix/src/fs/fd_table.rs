
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex};

use serde_derive::{Deserialize, Serialize};
use tokio::sync::{Mutex as AsyncMutex, watch};
use vfs_core::{VfsDirHandleAsync, VfsHandleAsync};
use wasmer_wasix_types::wasi::{EpollType, Fd as WasiFd, Fdflags, Fdflagsext, Rights};

use crate::net::socket::InodeSocket;

use super::notification::NotificationInner;
use super::pipes::{DuplexPipe, PipeRx, PipeTx};
use super::stdio::Stdio;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct FdEntry {
    #[cfg_attr(feature = "enable-serde", serde(flatten))]
    pub inner: FdInner,
    pub kind: Kind,
    pub is_stdio: bool,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct FdInner {
    pub rights: Rights,
    pub rights_inheriting: Rights,
    pub flags: Fdflags,
    pub fd_flags: Fdflagsext,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct EpollFd {
    pub events: EpollType,
    pub ptr: u64,
    pub fd: WasiFd,
    pub data1: u32,
    pub data2: u64,
}

#[derive(Debug, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct EpollInterest {
    pub interest: HashSet<(WasiFd, EpollType)>,
}

pub type EpollSubscriptions = HashMap<WasiFd, (EpollFd, Vec<EpollJoinGuard>)>;

#[derive(Debug)]
pub struct EpollJoinGuard {
    pub(crate) fd_guard: super::poll::InodeValFilePollGuard,
}

impl Drop for EpollJoinGuard {
    fn drop(&mut self) {
        self.fd_guard.cleanup();
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum Kind {
    VfsFile {
        handle: Arc<VfsHandleAsync>,
    },
    VfsDir {
        handle: VfsDirHandleAsync,
    },
    Stdin {
        handle: Arc<Stdio>,
    },
    Stdout {
        handle: Arc<Stdio>,
    },
    Stderr {
        handle: Arc<Stdio>,
    },
    PipeTx {
        tx: Arc<PipeTx>,
    },
    PipeRx {
        rx: Arc<PipeRx>,
    },
    DuplexPipe {
        pipe: Arc<DuplexPipe>,
    },
    Socket {
        socket: InodeSocket,
    },
    Epoll {
        subscriptions: Arc<StdMutex<EpollSubscriptions>>,
        tx: Arc<watch::Sender<EpollInterest>>,
        rx: Arc<AsyncMutex<watch::Receiver<EpollInterest>>>,
    },
    EventNotifications {
        inner: Arc<NotificationInner>,
    },
    Buffer {
        buffer: Vec<u8>,
    },
}

#[derive(Debug, Clone)]
pub struct FdTable {
    fds: Vec<Option<FdEntry>>,
    first_free: Option<usize>,
}

impl Default for FdTable {
    fn default() -> Self {
        Self::new()
    }
}

impl FdTable {
    pub fn new() -> Self {
        Self {
            fds: Vec::new(),
            first_free: None,
        }
    }

    pub fn next_free_fd(&self) -> WasiFd {
        match self.first_free {
            Some(i) => i as WasiFd,
            None => self.last_fd().map(|i| i + 1).unwrap_or(0),
        }
    }

    pub fn last_fd(&self) -> Option<WasiFd> {
        self.fds
            .iter()
            .rev()
            .position(|fd| fd.is_some())
            .map(|idx| (self.fds.len() - idx - 1) as WasiFd)
    }

    pub fn get(&self, idx: WasiFd) -> Option<&FdEntry> {
        self.fds.get(idx as usize).and_then(|x| x.as_ref())
    }

    pub fn get_mut(&mut self, idx: WasiFd) -> Option<&mut FdInner> {
        self.fds
            .get_mut(idx as usize)
            .and_then(|x| x.as_mut())
            .map(|x| &mut x.inner)
    }

    pub fn get_entry_mut(&mut self, idx: WasiFd) -> Option<&mut FdEntry> {
        self.fds.get_mut(idx as usize).and_then(|x| x.as_mut())
    }

    pub fn insert_first_free(&mut self, fd: FdEntry) -> WasiFd {
        match self.first_free {
            Some(free) => {
                self.fds[free] = Some(fd);
                self.first_free = self.first_free_after(free as WasiFd + 1);
                free as WasiFd
            }
            None => {
                self.fds.push(Some(fd));
                (self.fds.len() - 1) as WasiFd
            }
        }
    }

    pub fn insert_first_free_after(&mut self, fd: FdEntry, after_or_equal: WasiFd) -> WasiFd {
        match self.first_free {
            _ if self.fds.len() < after_or_equal as usize => {
                if !self.insert(true, after_or_equal, fd) {
                    panic!("FdTable: expected free slot at {after_or_equal}");
                }
                after_or_equal
            }
            Some(free) if free >= after_or_equal as usize => self.insert_first_free(fd),
            None if self.fds.len() >= after_or_equal as usize => self.insert_first_free(fd),
            None => unreachable!("FdTable: invalid state"),
            Some(_) => match self.first_free_after(after_or_equal) {
                Some(free) => {
                    self.fds[free] = Some(fd);
                    free as WasiFd
                }
                None => {
                    self.fds.push(Some(fd));
                    (self.fds.len() - 1) as WasiFd
                }
            },
        }
    }

    fn first_free_after(&self, after_or_equal: WasiFd) -> Option<usize> {
        let skip = after_or_equal as usize;
        self.fds
            .iter()
            .skip(skip)
            .position(|fd| fd.is_none())
            .map(|idx| idx + skip)
    }

    pub fn insert(&mut self, exclusive: bool, idx: WasiFd, fd: FdEntry) -> bool {
        let idx = idx as usize;
        if self.fds.len() <= idx {
            if self.first_free.is_none() && idx > self.fds.len() {
                self.first_free = Some(self.fds.len());
            }
            self.fds.resize(idx + 1, None);
        }

        if self.fds[idx].is_some() && exclusive {
            return false;
        }

        self.fds[idx] = Some(fd);

        if self.first_free == Some(idx) {
            self.first_free = self.first_free_after(idx as WasiFd + 1);
        }

        true
    }

    pub fn remove(&mut self, idx: WasiFd) -> Option<FdEntry> {
        let idx = idx as usize;
        let result = self.fds.get_mut(idx).and_then(|fd| fd.take());

        if result.is_some() {
            match self.first_free {
                None => self.first_free = Some(idx),
                Some(x) if x > idx => self.first_free = Some(idx),
                _ => (),
            }
        }

        result
    }

    pub fn clear(&mut self) {
        self.fds.clear();
        self.first_free = None;
    }

    pub fn iter(&self) -> impl Iterator<Item = (WasiFd, &FdEntry)> {
        self.fds
            .iter()
            .enumerate()
            .filter_map(|(idx, fd)| fd.as_ref().map(|fd| (idx as WasiFd, fd)))
    }

    pub fn keys(&self) -> impl Iterator<Item = WasiFd> + '_ {
        self.iter().map(|(key, _)| key)
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (WasiFd, &mut FdInner)> {
        self.fds
            .iter_mut()
            .enumerate()
            .filter_map(|(idx, fd)| fd.as_mut().map(|fd| (idx as WasiFd, &mut fd.inner)))
    }
}
