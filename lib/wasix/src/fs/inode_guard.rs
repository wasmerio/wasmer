use std::{
    future::Future,
    io::{IoSlice, SeekFrom},
    ops::{Deref, DerefMut},
    pin::Pin,
    sync::{Arc, RwLock},
    task::{Context, Poll},
};

use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};
use virtual_fs::{FsError, PipeRx, PipeTx, VirtualFile};
use wasmer_wasix_types::{
    types::Eventtype,
    wasi::{self, EpollType},
    wasi::{Errno, EventFdReadwrite, Eventrwflags, Subscription},
};

use super::{notification::NotificationInner, InodeGuard, Kind};
use crate::{
    net::socket::{InodeSocketInner, InodeSocketKind},
    state::{iterate_poll_events, PollEvent, PollEventSet, WasiState},
    syscalls::{map_io_err, EventResult, EventResultType},
    utils::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard},
};

#[derive(Debug, Clone)]
pub(crate) enum InodeValFilePollGuardMode {
    File(Arc<RwLock<Box<dyn VirtualFile + Send + Sync + 'static>>>),
    EventNotifications(Arc<NotificationInner>),
    Socket { inner: Arc<InodeSocketInner> },
    PipeRx { rx: Arc<RwLock<Box<PipeRx>>> },
    PipeTx { tx: Arc<RwLock<Box<PipeTx>>> },
}

pub struct InodeValFilePollGuard {
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
        let mode = match guard {
            Kind::EventNotifications { inner, .. } => {
                InodeValFilePollGuardMode::EventNotifications(inner.clone())
            }
            Kind::Socket { socket, .. } => InodeValFilePollGuardMode::Socket {
                inner: socket.inner.clone(),
            },
            Kind::File {
                handle: Some(handle),
                ..
            } => InodeValFilePollGuardMode::File(handle.clone()),
            Kind::PipeRx { rx } => InodeValFilePollGuardMode::PipeRx {
                rx: Arc::new(RwLock::new(Box::new(rx.clone()))),
            },
            Kind::PipeTx { tx } => InodeValFilePollGuardMode::PipeTx {
                tx: Arc::new(RwLock::new(Box::new(tx.clone()))),
            },
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
            InodeValFilePollGuardMode::File(..) => {
                write!(f, "guard-file(fd={}, peb={})", self.fd, self.peb)
            }
            InodeValFilePollGuardMode::EventNotifications { .. } => {
                write!(f, "guard-notifications(fd={}, peb={})", self.fd, self.peb)
            }
            InodeValFilePollGuardMode::Socket { inner } => {
                let inner = inner.protected.read().unwrap();
                match inner.kind {
                    InodeSocketKind::TcpListener { .. } => {
                        write!(f, "guard-tcp-listener(fd={}, peb={})", self.fd, self.peb)
                    }
                    InodeSocketKind::TcpStream { ref socket, .. } => {
                        if socket.is_closed() {
                            write!(
                                f,
                                "guard-tcp-stream (closed, fd={}, peb={})",
                                self.fd, self.peb
                            )
                        } else {
                            write!(f, "guard-tcp-stream(fd={}, peb={})", self.fd, self.peb)
                        }
                    }
                    InodeSocketKind::UdpSocket { .. } => {
                        write!(f, "guard-udp-socket(fd={}, peb={})", self.fd, self.peb)
                    }
                    InodeSocketKind::Raw(..) => {
                        write!(f, "guard-raw-socket(fd={}, peb={})", self.fd, self.peb)
                    }
                    _ => write!(f, "guard-socket(fd={}), peb={})", self.fd, self.peb),
                }
            }
            InodeValFilePollGuardMode::PipeRx { .. } => {
                write!(f, "guard-pipe-rx(...)")
            }
            InodeValFilePollGuardMode::PipeTx { .. } => {
                write!(f, "guard-pipe-tx(...)")
            }
        }
    }
}

#[derive(Debug)]
pub struct InodeValFilePollGuardJoin {
    mode: InodeValFilePollGuardMode,
    fd: u32,
    peb: PollEventSet,
    subscription: Subscription,
    spent: bool,
}

impl InodeValFilePollGuardJoin {
    pub(crate) fn new(guard: InodeValFilePollGuard) -> Self {
        Self {
            mode: guard.mode,
            fd: guard.fd,
            peb: guard.peb,
            subscription: guard.subscription,
            spent: false,
        }
    }
    pub(crate) fn fd(&self) -> u32 {
        self.fd
    }
    pub(crate) fn peb(&self) -> PollEventSet {
        self.peb
    }
    pub fn is_spent(&self) -> bool {
        self.spent
    }
    pub fn reset(&mut self) {
        match &self.mode {
            InodeValFilePollGuardMode::File(_) => {}
            InodeValFilePollGuardMode::EventNotifications(inner) => {
                inner.reset();
            }
            InodeValFilePollGuardMode::Socket { .. }
            | InodeValFilePollGuardMode::PipeRx { .. }
            | InodeValFilePollGuardMode::PipeTx { .. } => {}
        }
        self.spent = false;
    }
}

pub const POLL_GUARD_MAX_RET: usize = 4;

impl Future for InodeValFilePollGuardJoin {
    type Output = heapless::Vec<(EventResult, EpollType), POLL_GUARD_MAX_RET>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        // Otherwise we need to register for the event
        let waker = cx.waker();
        let mut has_read = false;
        let mut has_write = false;
        let mut has_close = false;
        let mut has_hangup = false;

        let mut ret = heapless::Vec::new();
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
        if has_read {
            let poll_result = match &mut self.mode {
                InodeValFilePollGuardMode::File(file) => {
                    let mut guard = file.write().unwrap();
                    let file = Pin::new(guard.as_mut());
                    file.poll_read_ready(cx)
                }
                InodeValFilePollGuardMode::EventNotifications(inner) => inner.poll(waker).map(Ok),
                InodeValFilePollGuardMode::Socket { ref inner } => {
                    let mut guard = inner.protected.write().unwrap();
                    guard.poll_read_ready(cx)
                }
                InodeValFilePollGuardMode::PipeRx { rx } => {
                    let mut guard = rx.write().unwrap();
                    let rx = Pin::new(guard.as_mut());
                    rx.poll_read_ready(cx)
                }
                InodeValFilePollGuardMode::PipeTx { .. } => Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Cannot read from a pipe write end",
                ))),
            };
            match poll_result {
                Poll::Ready(Err(err)) if has_close && is_err_closed(&err) => {
                    let inner = match self.subscription.type_ {
                        Eventtype::FdRead | Eventtype::FdWrite => {
                            Some(EventResultType::Fd(EventFdReadwrite {
                                nbytes: 0,
                                flags: if has_hangup {
                                    Eventrwflags::FD_READWRITE_HANGUP
                                } else {
                                    Eventrwflags::empty()
                                },
                            }))
                        }
                        Eventtype::Clock => Some(EventResultType::Clock(0)),
                        Eventtype::Unknown => None,
                    };
                    if let Some(inner) = inner {
                        ret.push((
                            EventResult {
                                userdata: self.subscription.userdata,
                                error: Errno::Success,
                                type_: self.subscription.type_,
                                inner,
                            },
                            EpollType::EPOLLHUP,
                        ))
                        .ok();
                    }
                }
                Poll::Ready(bytes_available) => {
                    let mut error = Errno::Success;
                    let bytes_available = match bytes_available {
                        Ok(a) => a,
                        Err(e) => {
                            error = map_io_err(e);
                            0
                        }
                    };
                    let inner = match self.subscription.type_ {
                        Eventtype::FdRead | Eventtype::FdWrite => {
                            Some(EventResultType::Fd(EventFdReadwrite {
                                nbytes: bytes_available as u64,
                                flags: if bytes_available == 0 {
                                    Eventrwflags::FD_READWRITE_HANGUP
                                } else {
                                    Eventrwflags::empty()
                                },
                            }))
                        }
                        Eventtype::Clock => Some(EventResultType::Clock(0)),
                        Eventtype::Unknown => None,
                    };
                    if let Some(inner) = inner {
                        ret.push((
                            EventResult {
                                userdata: self.subscription.userdata,
                                error,
                                type_: self.subscription.type_,
                                inner,
                            },
                            if error == Errno::Success {
                                EpollType::EPOLLIN
                            } else {
                                EpollType::EPOLLERR
                            },
                        ))
                        .ok();
                    }
                }
                Poll::Pending => {}
            };
        }
        if has_write {
            let poll_result = match &mut self.mode {
                InodeValFilePollGuardMode::File(file) => {
                    let mut guard = file.write().unwrap();
                    let file = Pin::new(guard.as_mut());
                    file.poll_write_ready(cx)
                }
                InodeValFilePollGuardMode::EventNotifications(inner) => inner.poll(waker).map(Ok),
                InodeValFilePollGuardMode::Socket { ref inner } => {
                    let mut guard = inner.protected.write().unwrap();
                    guard.poll_write_ready(cx)
                }
                InodeValFilePollGuardMode::PipeTx { tx } => {
                    let mut guard = tx.write().unwrap();
                    let tx = Pin::new(guard.as_mut());
                    tx.poll_write_ready()
                }
                InodeValFilePollGuardMode::PipeRx { .. } => Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Cannot write to a pipe read end",
                ))),
            };
            match poll_result {
                Poll::Ready(Err(err)) if has_close && is_err_closed(&err) => {
                    let inner = match self.subscription.type_ {
                        Eventtype::FdRead | Eventtype::FdWrite => {
                            Some(EventResultType::Fd(EventFdReadwrite {
                                nbytes: 0,
                                flags: if has_hangup {
                                    Eventrwflags::FD_READWRITE_HANGUP
                                } else {
                                    Eventrwflags::empty()
                                },
                            }))
                        }
                        Eventtype::Clock => Some(EventResultType::Clock(0)),
                        Eventtype::Unknown => None,
                    };
                    if let Some(inner) = inner {
                        ret.push((
                            EventResult {
                                userdata: self.subscription.userdata,
                                error: Errno::Success,
                                type_: self.subscription.type_,
                                inner,
                            },
                            EpollType::EPOLLHUP,
                        ))
                        .ok();
                    }
                }
                Poll::Ready(bytes_available) => {
                    let mut error = Errno::Success;
                    let bytes_available = match bytes_available {
                        Ok(a) => a,
                        Err(e) => {
                            error = map_io_err(e);
                            0
                        }
                    };
                    let inner = match self.subscription.type_ {
                        Eventtype::FdRead | Eventtype::FdWrite => {
                            Some(EventResultType::Fd(EventFdReadwrite {
                                nbytes: bytes_available as u64,
                                flags: if bytes_available == 0 {
                                    Eventrwflags::FD_READWRITE_HANGUP
                                } else {
                                    Eventrwflags::empty()
                                },
                            }))
                        }
                        Eventtype::Clock => Some(EventResultType::Clock(0)),
                        Eventtype::Unknown => None,
                    };
                    if let Some(inner) = inner {
                        ret.push((
                            EventResult {
                                userdata: self.subscription.userdata,
                                error,
                                type_: self.subscription.type_,
                                inner,
                            },
                            if error == Errno::Success {
                                EpollType::EPOLLOUT
                            } else {
                                EpollType::EPOLLERR
                            },
                        ))
                        .ok();
                    }
                }
                Poll::Pending => {}
            };
        }
        if !ret.is_empty() {
            self.spent = true;
            return Poll::Ready(ret);
        }
        Poll::Pending
    }
}

#[derive(Debug)]
pub(crate) struct InodeValFileReadGuard {
    guard: OwnedRwLockReadGuard<Box<dyn VirtualFile + Send + Sync + 'static>>,
}

impl InodeValFileReadGuard {
    pub(crate) fn new(file: &Arc<RwLock<Box<dyn VirtualFile + Send + Sync + 'static>>>) -> Self {
        Self {
            guard: crate::utils::read_owned(file).unwrap(),
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
            mode: InodeValFilePollGuardMode::File(self.guard.into_inner()),
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
    guard: OwnedRwLockWriteGuard<Box<dyn VirtualFile + Send + Sync + 'static>>,
}

impl InodeValFileWriteGuard {
    pub(crate) fn new(file: &Arc<RwLock<Box<dyn VirtualFile + Send + Sync + 'static>>>) -> Self {
        Self {
            guard: crate::utils::write_owned(file).unwrap(),
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
    inode: InodeGuard,
}

impl WasiStateFileGuard {
    pub fn new(state: &WasiState, fd: wasi::Fd) -> Result<Option<Self>, FsError> {
        let fd_map = state.fs.fd_map.read().unwrap();
        if let Some(fd) = fd_map.get(fd) {
            Ok(Some(Self {
                inode: fd.inode.clone(),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn lock_read(&self) -> Option<InodeValFileReadGuard> {
        let guard = self.inode.read();
        if let Kind::File { handle, .. } = guard.deref() {
            handle.as_ref().map(InodeValFileReadGuard::new)
        } else {
            // Our public API should ensure that this is not possible
            unreachable!("Non-file found in standard device location")
        }
    }

    pub fn lock_write(&self) -> Option<InodeValFileWriteGuard> {
        let guard = self.inode.read();
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
        let guard = self.lock_read();
        if let Some(file) = guard.as_ref() {
            file.last_accessed()
        } else {
            0
        }
    }

    fn last_modified(&self) -> u64 {
        let guard = self.lock_read();
        if let Some(file) = guard.as_ref() {
            file.last_modified()
        } else {
            0
        }
    }

    fn created_time(&self) -> u64 {
        let guard = self.lock_read();
        if let Some(file) = guard.as_ref() {
            file.created_time()
        } else {
            0
        }
    }

    fn set_times(
        &mut self,
        atime: Option<u64>,
        mtime: Option<u64>,
    ) -> Result<(), virtual_fs::FsError> {
        let mut guard = self.lock_write();
        if let Some(file) = guard.as_mut() {
            file.set_times(atime, mtime)
        } else {
            Err(crate::FsError::Lock)
        }
    }

    fn size(&self) -> u64 {
        let guard = self.lock_read();
        if let Some(file) = guard.as_ref() {
            file.size()
        } else {
            0
        }
    }

    fn set_len(&mut self, new_size: u64) -> Result<(), FsError> {
        let mut guard = self.lock_write();
        if let Some(file) = guard.as_mut() {
            file.set_len(new_size)
        } else {
            Err(FsError::IOError)
        }
    }

    fn unlink(&mut self) -> Result<(), FsError> {
        let mut guard = self.lock_write();
        if let Some(file) = guard.as_mut() {
            file.unlink()
        } else {
            Err(FsError::IOError)
        }
    }

    fn is_open(&self) -> bool {
        let guard = self.lock_read();
        if let Some(file) = guard.as_ref() {
            file.is_open()
        } else {
            false
        }
    }

    fn poll_read_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<usize>> {
        let mut guard = self.lock_write();
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
        let mut guard = self.lock_write();
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
        let mut guard = self.lock_write();
        if let Some(guard) = guard.as_mut() {
            let file = Pin::new(guard.deref_mut());
            file.start_seek(position)
        } else {
            Err(std::io::ErrorKind::Unsupported.into())
        }
    }
    fn poll_complete(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<u64>> {
        let mut guard = self.lock_write();
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
        let mut guard = self.lock_write();
        if let Some(guard) = guard.as_mut() {
            let file = Pin::new(guard.deref_mut());
            file.poll_write(cx, buf)
        } else {
            Poll::Ready(Err(std::io::ErrorKind::Unsupported.into()))
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let mut guard = self.lock_write();
        if let Some(guard) = guard.as_mut() {
            let file = Pin::new(guard.deref_mut());
            file.poll_flush(cx)
        } else {
            Poll::Ready(Err(std::io::ErrorKind::Unsupported.into()))
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let mut guard = self.lock_write();
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
        let mut guard = self.lock_write();
        if let Some(guard) = guard.as_mut() {
            let file = Pin::new(guard.deref_mut());
            file.poll_write_vectored(cx, bufs)
        } else {
            Poll::Ready(Err(std::io::ErrorKind::Unsupported.into()))
        }
    }
    fn is_write_vectored(&self) -> bool {
        let mut guard = self.lock_write();
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
        let mut guard = self.lock_write();
        if let Some(guard) = guard.as_mut() {
            let file = Pin::new(guard.deref_mut());
            file.poll_read(cx, buf)
        } else {
            Poll::Ready(Err(std::io::ErrorKind::Unsupported.into()))
        }
    }
}

fn is_err_closed(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::ConnectionAborted
        || err.kind() == std::io::ErrorKind::ConnectionRefused
        || err.kind() == std::io::ErrorKind::ConnectionReset
        || err.kind() == std::io::ErrorKind::BrokenPipe
        || err.kind() == std::io::ErrorKind::NotConnected
        || err.kind() == std::io::ErrorKind::UnexpectedEof
}
