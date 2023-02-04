use std::{
    future::Future,
    io::{IoSlice, SeekFrom},
    ops::{Deref, DerefMut},
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard,
    },
    task::{Context, Poll},
};

use tokio::{
    io::{AsyncRead, AsyncSeek, AsyncWrite},
    sync::mpsc,
};
use wasmer_vfs::{FsError, VirtualFile};
use wasmer_vnet::{net_error_into_io_err, NetworkError};
use wasmer_wasi_types::{
    types::Eventtype,
    wasi,
    wasi::{Errno, Event, EventFdReadwrite, EventUnion, Eventrwflags, Subscription},
};

use super::Kind;
use crate::{
    net::socket::{InodeSocketInner, InodeSocketKind},
    state::{iterate_poll_events, PollEvent, PollEventSet},
    syscalls::map_io_err,
    WasiInodes, WasiState,
};

pub(crate) enum InodeValFilePollGuardMode {
    File(Arc<RwLock<Box<dyn VirtualFile + Send + Sync + 'static>>>),
    EventNotifications {
        immediate: bool,
        waker: Mutex<mpsc::UnboundedReceiver<()>>,
        counter: Arc<AtomicU64>,
    },
    Socket {
        inner: Arc<RwLock<InodeSocketInner>>,
    },
}

pub(crate) struct InodeValFilePollGuard {
    pub(crate) fd: u32,
    pub(crate) peb: PollEventSet,
    pub(crate) subscription: Subscription,
    pub(crate) mode: InodeValFilePollGuardMode,
}

impl InodeValFilePollGuard {
    pub(crate) fn new(
        fd: u32,
        peb: PollEventSet,
        subscription: Subscription,
        guard: &Kind,
    ) -> Option<Self> {
        let mode = match guard.deref() {
            Kind::EventNotifications {
                counter,
                wakers,
                immediate,
                ..
            } => {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                let immediate = {
                    let mut wakers = wakers.lock().unwrap();
                    wakers.push_back(tx);
                    immediate
                        .compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed)
                        .is_ok()
                };
                InodeValFilePollGuardMode::EventNotifications {
                    immediate,
                    waker: Mutex::new(rx),
                    counter: counter.clone(),
                }
            }
            Kind::Socket { socket } => InodeValFilePollGuardMode::Socket {
                inner: socket.inner.clone(),
            },
            Kind::File {
                handle: Some(handle),
                ..
            } => InodeValFilePollGuardMode::File(handle.clone()),
            _ => {
                return None;
            }
        };
        Some(Self {
            fd,
            mode,
            peb,
            subscription,
        })
    }
}

impl std::fmt::Debug for InodeValFilePollGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.mode {
            InodeValFilePollGuardMode::File(..) => write!(f, "guard-file"),
            InodeValFilePollGuardMode::EventNotifications { .. } => {
                write!(f, "guard-notifications")
            }
            InodeValFilePollGuardMode::Socket { inner } => {
                let inner = inner.read().unwrap();
                match inner.kind {
                    InodeSocketKind::TcpListener { .. } => write!(f, "guard-tcp-listener"),
                    InodeSocketKind::TcpStream { ref socket, .. } => {
                        if socket.is_closed() {
                            write!(f, "guard-tcp-stream (closed)")
                        } else {
                            write!(f, "guard-tcp-stream")
                        }
                    }
                    InodeSocketKind::UdpSocket { .. } => write!(f, "guard-udp-socket"),
                    InodeSocketKind::Raw(..) => write!(f, "guard-raw-socket"),
                    InodeSocketKind::WebSocket(..) => write!(f, "guard-web-socket"),
                    _ => write!(f, "guard-socket"),
                }
            }
        }
    }
}

impl InodeValFilePollGuard {
    #[allow(dead_code)]
    pub fn is_open(&self) -> bool {
        match &self.mode {
            InodeValFilePollGuardMode::File(file) => {
                let guard = file.read().unwrap();
                guard.is_open()
            }
            InodeValFilePollGuardMode::EventNotifications { .. }
            | InodeValFilePollGuardMode::Socket { .. } => true,
        }
    }
}

pub(crate) struct InodeValFilePollGuardJoin<'a> {
    mode: &'a mut InodeValFilePollGuardMode,
    fd: u32,
    peb: PollEventSet,
    subscription: Subscription,
}

impl<'a> InodeValFilePollGuardJoin<'a> {
    pub(crate) fn new(guard: &'a mut InodeValFilePollGuard) -> Self {
        Self {
            mode: &mut guard.mode,
            fd: guard.fd,
            peb: guard.peb,
            subscription: guard.subscription,
        }
    }
    pub(crate) fn fd(&self) -> u32 {
        self.fd
    }
}

impl<'a> Future for InodeValFilePollGuardJoin<'a> {
    type Output = Event;

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let mut has_read = false;
        let mut has_write = false;
        let mut has_close = false;
        let mut has_hangup = false;

        for in_event in iterate_poll_events(self.peb) {
            match in_event {
                PollEvent::PollIn => {
                    has_read = true;
                }
                PollEvent::PollOut => {
                    has_write = true;
                }
                PollEvent::PollHangUp => {
                    has_hangup = true;
                    has_close = true;
                }
                PollEvent::PollError | PollEvent::PollInvalid => {
                    if !has_hangup {
                        has_close = true;
                    }
                }
            }
        }
        if has_close {
            let is_closed = match &mut self.mode {
                InodeValFilePollGuardMode::File(file) => {
                    let mut guard = file.write().unwrap();
                    let file = Pin::new(guard.as_mut());
                    file.poll_shutdown(cx).is_ready()
                }
                InodeValFilePollGuardMode::EventNotifications { .. } => false,
                InodeValFilePollGuardMode::Socket { ref inner } => {
                    let mut guard = inner.write().unwrap();
                    let is_closed = if let InodeSocketKind::Closed = guard.kind {
                        true
                    } else if has_read || has_write {
                        // this will be handled in the read/write poll instead
                        false
                    } else {
                        // we do a read poll which will error out if its closed
                        #[allow(clippy::match_like_matches_macro)]
                        match guard.poll_read_ready(cx) {
                            Poll::Ready(Ok(0)) => true,
                            Poll::Ready(Err(NetworkError::ConnectionAborted))
                            | Poll::Ready(Err(NetworkError::ConnectionRefused))
                            | Poll::Ready(Err(NetworkError::ConnectionReset))
                            | Poll::Ready(Err(NetworkError::BrokenPipe))
                            | Poll::Ready(Err(NetworkError::NotConnected))
                            | Poll::Ready(Err(NetworkError::UnexpectedEof)) => true,
                            _ => false,
                        }
                    };
                    is_closed
                }
            };
            if is_closed {
                return Poll::Ready(Event {
                    userdata: self.subscription.userdata,
                    error: Errno::Success,
                    type_: self.subscription.type_,
                    u: match self.subscription.type_ {
                        Eventtype::FdRead | Eventtype::FdWrite => EventUnion {
                            fd_readwrite: EventFdReadwrite {
                                nbytes: 0,
                                flags: if has_hangup {
                                    Eventrwflags::FD_READWRITE_HANGUP
                                } else {
                                    Eventrwflags::empty()
                                },
                            },
                        },
                        Eventtype::Clock => EventUnion { clock: 0 },
                    },
                });
            }
        }
        if has_read {
            let mut poll_result = match &mut self.mode {
                InodeValFilePollGuardMode::File(file) => {
                    let mut guard = file.write().unwrap();
                    let file = Pin::new(guard.as_mut());
                    file.poll_read_ready(cx)
                }
                InodeValFilePollGuardMode::EventNotifications {
                    waker,
                    counter,
                    immediate,
                    ..
                } => {
                    if *immediate {
                        let cnt = counter.load(Ordering::Acquire);
                        Poll::Ready(Ok(cnt as usize))
                    } else {
                        let counter = counter.clone();
                        let mut waker = waker.lock().unwrap();
                        let mut notifications = Pin::new(waker.deref_mut());
                        notifications.poll_recv(cx).map(|_| {
                            let cnt = counter.load(Ordering::Acquire);
                            Ok(cnt as usize)
                        })
                    }
                }
                InodeValFilePollGuardMode::Socket { ref inner } => {
                    let mut guard = inner.write().unwrap();
                    guard.poll_read_ready(cx).map_err(net_error_into_io_err)
                }
            };
            if has_close {
                poll_result = match poll_result {
                    Poll::Ready(Err(err))
                        if err.kind() == std::io::ErrorKind::ConnectionAborted
                            || err.kind() == std::io::ErrorKind::ConnectionRefused
                            || err.kind() == std::io::ErrorKind::ConnectionReset
                            || err.kind() == std::io::ErrorKind::BrokenPipe
                            || err.kind() == std::io::ErrorKind::NotConnected
                            || err.kind() == std::io::ErrorKind::UnexpectedEof =>
                    {
                        return Poll::Ready(Event {
                            userdata: self.subscription.userdata,
                            error: Errno::Success,
                            type_: self.subscription.type_,
                            u: match self.subscription.type_ {
                                Eventtype::FdRead | Eventtype::FdWrite => EventUnion {
                                    fd_readwrite: EventFdReadwrite {
                                        nbytes: 0,
                                        flags: if has_hangup {
                                            Eventrwflags::FD_READWRITE_HANGUP
                                        } else {
                                            Eventrwflags::empty()
                                        },
                                    },
                                },
                                Eventtype::Clock => EventUnion { clock: 0 },
                            },
                        });
                    }
                    a => a,
                };
            }
            if let Poll::Ready(bytes_available) = poll_result {
                let mut error = Errno::Success;
                let bytes_available = match bytes_available {
                    Ok(a) => a,
                    Err(e) => {
                        error = map_io_err(e);
                        0
                    }
                };
                return Poll::Ready(Event {
                    userdata: self.subscription.userdata,
                    error,
                    type_: self.subscription.type_,
                    u: match self.subscription.type_ {
                        Eventtype::FdRead | Eventtype::FdWrite => EventUnion {
                            fd_readwrite: EventFdReadwrite {
                                nbytes: bytes_available as u64,
                                flags: if bytes_available == 0 {
                                    Eventrwflags::FD_READWRITE_HANGUP
                                } else {
                                    Eventrwflags::empty()
                                },
                            },
                        },
                        Eventtype::Clock => EventUnion { clock: 0 },
                    },
                });
            }
        }
        if has_write {
            let mut poll_result = match &mut self.mode {
                InodeValFilePollGuardMode::File(file) => {
                    let mut guard = file.write().unwrap();
                    let file = Pin::new(guard.as_mut());
                    file.poll_write_ready(cx)
                }
                InodeValFilePollGuardMode::EventNotifications {
                    waker,
                    counter,
                    immediate,
                    ..
                } => {
                    if *immediate {
                        let cnt = counter.load(Ordering::Acquire);
                        Poll::Ready(Ok(cnt as usize))
                    } else {
                        let counter = counter.clone();
                        let mut waker = waker.lock().unwrap();
                        let mut notifications = Pin::new(waker.deref_mut());
                        notifications.poll_recv(cx).map(|_| {
                            let cnt = counter.load(Ordering::Acquire);
                            Ok(cnt as usize)
                        })
                    }
                }
                InodeValFilePollGuardMode::Socket { ref inner } => {
                    let mut guard = inner.write().unwrap();
                    guard.poll_write_ready(cx).map_err(net_error_into_io_err)
                }
            };
            if has_close {
                poll_result = match poll_result {
                    Poll::Ready(Err(err))
                        if err.kind() == std::io::ErrorKind::ConnectionAborted
                            || err.kind() == std::io::ErrorKind::ConnectionRefused
                            || err.kind() == std::io::ErrorKind::ConnectionReset
                            || err.kind() == std::io::ErrorKind::BrokenPipe
                            || err.kind() == std::io::ErrorKind::NotConnected
                            || err.kind() == std::io::ErrorKind::UnexpectedEof =>
                    {
                        return Poll::Ready(Event {
                            userdata: self.subscription.userdata,
                            error: Errno::Success,
                            type_: self.subscription.type_,
                            u: match self.subscription.type_ {
                                Eventtype::FdRead | Eventtype::FdWrite => EventUnion {
                                    fd_readwrite: EventFdReadwrite {
                                        nbytes: 0,
                                        flags: if has_hangup {
                                            Eventrwflags::FD_READWRITE_HANGUP
                                        } else {
                                            Eventrwflags::empty()
                                        },
                                    },
                                },
                                Eventtype::Clock => EventUnion { clock: 0 },
                            },
                        });
                    }
                    a => a,
                };
            }
            if let Poll::Ready(bytes_available) = poll_result {
                let mut error = Errno::Success;
                let bytes_available = match bytes_available {
                    Ok(a) => a,
                    Err(e) => {
                        error = map_io_err(e);
                        0
                    }
                };
                return Poll::Ready(Event {
                    userdata: self.subscription.userdata,
                    error,
                    type_: self.subscription.type_,
                    u: match self.subscription.type_ {
                        Eventtype::FdRead | Eventtype::FdWrite => EventUnion {
                            fd_readwrite: EventFdReadwrite {
                                nbytes: bytes_available as u64,
                                flags: if bytes_available == 0 {
                                    Eventrwflags::FD_READWRITE_HANGUP
                                } else {
                                    Eventrwflags::empty()
                                },
                            },
                        },
                        Eventtype::Clock => EventUnion { clock: 0 },
                    },
                });
            }
        }
        Poll::Pending
    }
}

#[derive(Debug)]
pub(crate) struct InodeValFileReadGuard {
    #[allow(dead_code)]
    file: Arc<RwLock<Box<dyn VirtualFile + Send + Sync + 'static>>>,
    guard: RwLockReadGuard<'static, Box<dyn VirtualFile + Send + Sync + 'static>>,
}

impl InodeValFileReadGuard {
    pub(crate) fn new(file: &Arc<RwLock<Box<dyn VirtualFile + Send + Sync + 'static>>>) -> Self {
        let guard = file.read().unwrap();
        Self {
            file: file.clone(),
            guard: unsafe { std::mem::transmute(guard) },
        }
    }
}

impl InodeValFileReadGuard {
    pub fn into_poll_guard(
        self,
        fd: u32,
        peb: PollEventSet,
        subscription: Subscription,
    ) -> InodeValFilePollGuard {
        InodeValFilePollGuard {
            fd,
            peb,
            subscription,
            mode: InodeValFilePollGuardMode::File(self.file),
        }
    }
}

impl Deref for InodeValFileReadGuard {
    type Target = dyn VirtualFile + Send + Sync + 'static;
    fn deref(&self) -> &Self::Target {
        self.guard.deref().deref()
    }
}

#[derive(Debug)]
pub struct InodeValFileWriteGuard {
    #[allow(dead_code)]
    file: Arc<RwLock<Box<dyn VirtualFile + Send + Sync + 'static>>>,
    guard: RwLockWriteGuard<'static, Box<dyn VirtualFile + Send + Sync + 'static>>,
}

impl InodeValFileWriteGuard {
    pub(crate) fn new(file: &Arc<RwLock<Box<dyn VirtualFile + Send + Sync + 'static>>>) -> Self {
        let guard = file.write().unwrap();
        Self {
            file: file.clone(),
            guard: unsafe { std::mem::transmute(guard) },
        }
    }
    pub(crate) fn swap(
        &mut self,
        mut file: Box<dyn VirtualFile + Send + Sync + 'static>,
    ) -> Box<dyn VirtualFile + Send + Sync + 'static> {
        std::mem::swap(self.guard.deref_mut(), &mut file);
        file
    }
}

impl Deref for InodeValFileWriteGuard {
    type Target = dyn VirtualFile + Send + Sync + 'static;
    fn deref(&self) -> &Self::Target {
        self.guard.deref().deref()
    }
}
impl DerefMut for InodeValFileWriteGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.deref_mut().deref_mut()
    }
}

#[derive(Debug)]
pub(crate) struct WasiStateFileGuard {
    inodes: Arc<RwLock<WasiInodes>>,
    inode: generational_arena::Index,
}

impl WasiStateFileGuard {
    pub fn new(state: &WasiState, fd: wasi::Fd) -> Result<Option<Self>, FsError> {
        let inodes = state.inodes.read().unwrap();
        let fd_map = state.fs.fd_map.read().unwrap();
        if let Some(fd) = fd_map.get(&fd) {
            let guard = inodes.arena[fd.inode].read();
            if let Kind::File { .. } = guard.deref() {
                Ok(Some(Self {
                    inodes: state.inodes.clone(),
                    inode: fd.inode,
                }))
            } else {
                // Our public API should ensure that this is not possible
                Err(FsError::NotAFile)
            }
        } else {
            Ok(None)
        }
    }

    pub fn lock_read(&self, inodes: &RwLockReadGuard<WasiInodes>) -> Option<InodeValFileReadGuard> {
        let guard = inodes.arena[self.inode].read();
        if let Kind::File { handle, .. } = guard.deref() {
            handle.as_ref().map(InodeValFileReadGuard::new)
        } else {
            // Our public API should ensure that this is not possible
            unreachable!("Non-file found in standard device location")
        }
    }

    pub fn lock_write(
        &self,
        inodes: &RwLockReadGuard<WasiInodes>,
    ) -> Option<InodeValFileWriteGuard> {
        let guard = inodes.arena[self.inode].read();
        if let Kind::File { handle, .. } = guard.deref() {
            handle.as_ref().map(InodeValFileWriteGuard::new)
        } else {
            // Our public API should ensure that this is not possible
            unreachable!("Non-file found in standard device location")
        }
    }
}

impl VirtualFile for WasiStateFileGuard {
    fn last_accessed(&self) -> u64 {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.as_ref() {
            file.last_accessed()
        } else {
            0
        }
    }

    fn last_modified(&self) -> u64 {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.as_ref() {
            file.last_modified()
        } else {
            0
        }
    }

    fn created_time(&self) -> u64 {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.as_ref() {
            file.created_time()
        } else {
            0
        }
    }

    fn size(&self) -> u64 {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.as_ref() {
            file.size()
        } else {
            0
        }
    }

    fn set_len(&mut self, new_size: u64) -> Result<(), FsError> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.as_mut() {
            file.set_len(new_size)
        } else {
            Err(FsError::IOError)
        }
    }

    fn unlink(&mut self) -> Result<(), FsError> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.as_mut() {
            file.unlink()
        } else {
            Err(FsError::IOError)
        }
    }

    fn is_open(&self) -> bool {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.as_ref() {
            file.is_open()
        } else {
            false
        }
    }

    fn poll_read_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<usize>> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.as_mut() {
            let file = Pin::new(file.deref_mut());
            file.poll_read_ready(cx)
        } else {
            Poll::Ready(Ok(0))
        }
    }

    fn poll_write_ready(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<usize>> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.as_mut() {
            let file = Pin::new(file.deref_mut());
            file.poll_write_ready(cx)
        } else {
            Poll::Ready(Ok(0))
        }
    }
}

impl AsyncSeek for WasiStateFileGuard {
    fn start_seek(self: Pin<&mut Self>, position: SeekFrom) -> std::io::Result<()> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(guard) = guard.as_mut() {
            let file = Pin::new(guard.deref_mut());
            file.start_seek(position)
        } else {
            Err(std::io::ErrorKind::Unsupported.into())
        }
    }
    fn poll_complete(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<u64>> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(guard) = guard.as_mut() {
            let file = Pin::new(guard.deref_mut());
            file.poll_complete(cx)
        } else {
            Poll::Ready(Err(std::io::ErrorKind::Unsupported.into()))
        }
    }
}

impl AsyncWrite for WasiStateFileGuard {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(guard) = guard.as_mut() {
            let file = Pin::new(guard.deref_mut());
            file.poll_write(cx, buf)
        } else {
            Poll::Ready(Err(std::io::ErrorKind::Unsupported.into()))
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(guard) = guard.as_mut() {
            let file = Pin::new(guard.deref_mut());
            file.poll_flush(cx)
        } else {
            Poll::Ready(Err(std::io::ErrorKind::Unsupported.into()))
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(guard) = guard.as_mut() {
            let file = Pin::new(guard.deref_mut());
            file.poll_shutdown(cx)
        } else {
            Poll::Ready(Err(std::io::ErrorKind::Unsupported.into()))
        }
    }
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(guard) = guard.as_mut() {
            let file = Pin::new(guard.deref_mut());
            file.poll_write_vectored(cx, bufs)
        } else {
            Poll::Ready(Err(std::io::ErrorKind::Unsupported.into()))
        }
    }
    fn is_write_vectored(&self) -> bool {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(guard) = guard.as_mut() {
            let file = Pin::new(guard.deref_mut());
            file.is_write_vectored()
        } else {
            false
        }
    }
}

impl AsyncRead for WasiStateFileGuard {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(guard) = guard.as_mut() {
            let file = Pin::new(guard.deref_mut());
            file.poll_read(cx, buf)
        } else {
            Poll::Ready(Err(std::io::ErrorKind::Unsupported.into()))
        }
    }
}
