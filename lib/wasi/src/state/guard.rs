use tokio::sync::mpsc;
use wasmer_vnet::{net_error_into_io_err, NetworkError};

use crate::VirtualTaskManager;

use super::*;
use std::{
    io::{Read, Seek},
    sync::RwLockReadGuard, future::Future, pin::Pin, task::Poll,
};

pub(crate) enum InodeValFilePollGuardMode {
    File(Arc<RwLock<Box<dyn VirtualFile + Send + Sync + 'static>>>),
    EventNotifications {
        immediate: bool,
        waker: Mutex<mpsc::UnboundedReceiver<()>>,
        counter: Arc<AtomicU64>,
        wakers: Arc<Mutex<VecDeque<tokio::sync::mpsc::UnboundedSender<()>>>>
    },
    Socket(InodeSocket)
}

pub(crate) struct InodeValFilePollGuard {
    pub(crate) fd: u32,
    pub(crate) mode: InodeValFilePollGuardMode,
    pub(crate) subscriptions: HashMap<PollEventSet, WasiSubscription>,
    pub(crate) tasks: Arc<dyn VirtualTaskManager + Send + Sync + 'static>,
}
impl<'a> InodeValFilePollGuard {
    pub(crate) fn new(fd: u32, guard: &Kind, subscriptions: HashMap<PollEventSet, WasiSubscription>, tasks: Arc<dyn VirtualTaskManager + Send + Sync + 'static>) -> Option<Self> {
        let mode = match guard.deref() {
            Kind::EventNotifications { counter, wakers, immediate, .. } => {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                let immediate = {
                    let mut wakers = wakers.lock().unwrap();
                    wakers.push_back(tx);
                    immediate.compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed).is_ok()
                };
                InodeValFilePollGuardMode::EventNotifications {
                    immediate,
                    waker: Mutex::new(rx),
                    counter: counter.clone(),
                    wakers: wakers.clone(),
                }
            },
            Kind::Socket { socket } => InodeValFilePollGuardMode::Socket(socket.clone()),
            Kind::File { handle, .. } => {
                if let Some(handle) = handle {
                    InodeValFilePollGuardMode::File(handle.clone())
                } else {
                    return None;
                }
            },
            _ => {
                return None;
            }
        };
        Some(
            Self {
                fd,
                mode,
                subscriptions,
                tasks
            }
        )
    }
}

impl std::fmt::Debug
for InodeValFilePollGuard
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.mode {
            InodeValFilePollGuardMode::File(..) => write!(f, "guard-file"),
            InodeValFilePollGuardMode::EventNotifications { .. } => write!(f, "guard-notifications"),
            InodeValFilePollGuardMode::Socket(socket) => {
                let socket = socket.inner.read().unwrap();
                match socket.kind {
                    InodeSocketKind::TcpListener(..) => write!(f, "guard-tcp-listener"),
                    InodeSocketKind::TcpStream(..) => write!(f, "guard-tcp-stream"),
                    InodeSocketKind::UdpSocket(..) => write!(f, "guard-udp-socket"),
                    InodeSocketKind::Raw(..) => write!(f, "guard-raw-socket"),
                    InodeSocketKind::HttpRequest(..) => write!(f, "guard-http-request"),
                    InodeSocketKind::WebSocket(..) => write!(f, "guard-web-socket"),
                    _ => write!(f, "guard-socket")
                }
            }
        }
    }
}

impl InodeValFilePollGuard {
    pub fn bytes_available_read(&self) -> wasmer_vfs::Result<Option<usize>> {
        match &self.mode {
            InodeValFilePollGuardMode::File(file) => {
                let guard = file.read().unwrap();
                guard.bytes_available_read()
            },
            InodeValFilePollGuardMode::EventNotifications { counter, .. } => {
                Ok(
                    Some(counter.load(std::sync::atomic::Ordering::Acquire) as usize)
                )
            },
            InodeValFilePollGuardMode::Socket(socket) => {
                socket.peek()
                    .map(|a| Some(a))
                    .map_err(fs_error_from_wasi_err)
            }
        }
    }

    pub fn bytes_available_write(&self) -> wasmer_vfs::Result<Option<usize>> {
        match &self.mode {
            InodeValFilePollGuardMode::File(file) => {
                let guard = file.read().unwrap();
                guard.bytes_available_write()
            },
            InodeValFilePollGuardMode::EventNotifications { wakers, .. } => {
                let wakers = wakers.lock().unwrap();
                Ok(
                    Some(wakers.len())
                )
            },
            InodeValFilePollGuardMode::Socket(socket) => {
                if socket.can_write() {
                    Ok(Some(4096))
                } else {
                    Ok(Some(0))
                }
            }
        }
    }

    pub fn is_open(&self) -> bool{
        match &self.mode {
            InodeValFilePollGuardMode::File(file) => {
                let guard = file.read().unwrap();
                guard.is_open()
            },
            InodeValFilePollGuardMode::EventNotifications { .. } |
            InodeValFilePollGuardMode::Socket(..) => {
                true
            }
        }
    }

    pub async fn wait(&self) -> Vec<__wasi_event_t> {
        InodeValFilePollGuardJoin::new(self).await
    }
}

struct InodeValFilePollGuardJoin<'a> {
    mode: &'a InodeValFilePollGuardMode,
    subscriptions: HashMap<PollEventSet, WasiSubscription>,
    tasks: Arc<dyn VirtualTaskManager + Send + Sync + 'static>,
}
impl<'a> InodeValFilePollGuardJoin<'a> {
    fn new(guard: &'a InodeValFilePollGuard) -> Self {
        Self {
            mode: &guard.mode,
            subscriptions: guard.subscriptions.clone(),
            tasks: guard.tasks.clone(),
        }
    }
}
impl<'a> Future
for InodeValFilePollGuardJoin<'a>
{
    type Output = Vec<__wasi_event_t>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let mut has_read = None;
        let mut has_write = None;
        let mut has_close = None;
        let mut has_hangup = false;

        let register_root_waker = self.tasks
            .register_root_waker();

        let mut ret = Vec::new();
        for (set, s) in self.subscriptions.iter() {
            for in_event in iterate_poll_events(*set) {
                match in_event {
                    PollEvent::PollIn => { has_read = Some(s.clone()); },
                    PollEvent::PollOut => { has_write = Some(s.clone()); },
                    PollEvent::PollHangUp => {
                        has_hangup = true;
                        has_close = Some(s.clone());
                    }
                    PollEvent::PollError |
                    PollEvent::PollInvalid => {
                        if has_hangup == false {
                            has_close = Some(s.clone());
                        }
                    }
                }
            }
        }
        if let Some(s) = has_close.as_ref() {
            let is_closed = match self.mode {
                InodeValFilePollGuardMode::File(file) => {
                    let guard = file.read().unwrap();
                    guard.poll_close_ready(cx, &register_root_waker).is_ready()
                },
                InodeValFilePollGuardMode::EventNotifications { .. } => {
                    false
                },
                InodeValFilePollGuardMode::Socket(socket) => {
                    let inner = socket.inner.read().unwrap();
                    if let InodeSocketKind::Closed = inner.kind {
                        true
                    } else {
                        if has_read.is_some() || has_write.is_some()
                        {
                            // this will be handled in the read/write poll instead
                            false
                        } else {
                            // we do a read poll which will error out if its closed
                            match socket.poll_read_ready(cx) {
                                Poll::Ready(Err(NetworkError::ConnectionAborted)) |
                                Poll::Ready(Err(NetworkError::ConnectionRefused)) |
                                Poll::Ready(Err(NetworkError::ConnectionReset)) |
                                Poll::Ready(Err(NetworkError::BrokenPipe)) |
                                Poll::Ready(Err(NetworkError::NotConnected)) |
                                Poll::Ready(Err(NetworkError::UnexpectedEof)) => {
                                    true
                                },
                                _ => {
                                    false
                                }
                            }
                        }                        
                    }
                }
            };
            if is_closed {
                ret.push(__wasi_event_t {
                    userdata: s.user_data,
                    error: __WASI_ESUCCESS,
                    type_: s.event_type.raw_tag(),
                    u: {
                        __wasi_event_u {
                            fd_readwrite: __wasi_event_fd_readwrite_t {
                                nbytes: 0,
                                flags: if has_hangup {
                                    __WASI_EVENT_FD_READWRITE_HANGUP
                                } else { 0 },
                            },
                        }
                    },
                });
            }
        }
        if let Some(s) = has_read {
            let mut poll_result = match &self.mode {
                InodeValFilePollGuardMode::File(file) => {
                    let guard = file.read().unwrap();
                    guard.poll_read_ready(cx, &register_root_waker)
                },
                InodeValFilePollGuardMode::EventNotifications { waker, counter, immediate, .. } => {
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
                },
                InodeValFilePollGuardMode::Socket(socket) => {
                    socket.poll_read_ready(cx)
                        .map_err(net_error_into_io_err)
                        .map_err(Into::<FsError>::into)
                }
            };
            if let Some(s) = has_close.as_ref() {
                poll_result = match poll_result {
                    Poll::Ready(Err(FsError::ConnectionAborted)) |
                    Poll::Ready(Err(FsError::ConnectionRefused)) |
                    Poll::Ready(Err(FsError::ConnectionReset)) |
                    Poll::Ready(Err(FsError::BrokenPipe)) |
                    Poll::Ready(Err(FsError::NotConnected)) |
                    Poll::Ready(Err(FsError::UnexpectedEof)) => {
                        ret.push(__wasi_event_t {
                            userdata: s.user_data,
                            error: __WASI_ESUCCESS,
                            type_: s.event_type.raw_tag(),
                            u: {
                                __wasi_event_u {
                                    fd_readwrite: __wasi_event_fd_readwrite_t {
                                        nbytes: 0,
                                        flags: if has_hangup {
                                            __WASI_EVENT_FD_READWRITE_HANGUP
                                        } else { 0 },
                                    },
                                }
                            },
                        });
                        Poll::Pending
                    }
                    a => a
                };
            }
            if let Poll::Ready(bytes_available) = poll_result {
                ret.push(__wasi_event_t {
                    userdata: s.user_data,
                    error: bytes_available.clone().map(|_| __WASI_ESUCCESS).unwrap_or_else(fs_error_into_wasi_err),
                    type_: s.event_type.raw_tag(),
                    u: {
                        __wasi_event_u {
                            fd_readwrite: __wasi_event_fd_readwrite_t {
                                nbytes: bytes_available.unwrap_or_default() as u64,
                                flags: 0,
                            },
                        }
                    },
                });
            }
        }
        if let Some(s) = has_write {
            let mut poll_result = match self.mode {
                InodeValFilePollGuardMode::File(file) => {
                    let guard = file.read().unwrap();
                    guard.poll_write_ready(cx, &register_root_waker)
                },
                InodeValFilePollGuardMode::EventNotifications { waker, counter, immediate, .. } => {
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
                },
                InodeValFilePollGuardMode::Socket(socket) => {
                    socket.poll_write_ready(cx)
                        .map_err(net_error_into_io_err)
                        .map_err(Into::<FsError>::into)
                }
            };
            if let Some(s) = has_close.as_ref() {
                poll_result = match poll_result {
                    Poll::Ready(Err(FsError::ConnectionAborted)) |
                    Poll::Ready(Err(FsError::ConnectionRefused)) |
                    Poll::Ready(Err(FsError::ConnectionReset)) |
                    Poll::Ready(Err(FsError::BrokenPipe)) |
                    Poll::Ready(Err(FsError::NotConnected)) |
                    Poll::Ready(Err(FsError::UnexpectedEof)) => {
                        ret.push(__wasi_event_t {
                            userdata: s.user_data,
                            error: __WASI_ESUCCESS,
                            type_: s.event_type.raw_tag(),
                            u: {
                                __wasi_event_u {
                                    fd_readwrite: __wasi_event_fd_readwrite_t {
                                        nbytes: 0,
                                        flags: if has_hangup {
                                            __WASI_EVENT_FD_READWRITE_HANGUP
                                        } else { 0 },
                                    },
                                }
                            },
                        });
                        Poll::Pending
                    }
                    a => a
                };
            }
            if let Poll::Ready(bytes_available) = poll_result {
                ret.push(__wasi_event_t {
                    userdata: s.user_data,
                    error: bytes_available.clone().map(|_| __WASI_ESUCCESS).unwrap_or_else(fs_error_into_wasi_err),
                    type_: s.event_type.raw_tag(),
                    u: {
                        __wasi_event_u {
                            fd_readwrite: __wasi_event_fd_readwrite_t {
                                nbytes: bytes_available.unwrap_or_default() as u64,
                                flags: 0,
                            },
                        }
                    },
                });
            }
        }
        if ret.len() > 0 {
            Poll::Ready(ret)
        } else {
            Poll::Pending
        }
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
            guard: unsafe { std::mem::transmute(guard) }
        }
    }
}

impl InodeValFileReadGuard {
    pub fn into_poll_guard(self, fd: u32, subscriptions: HashMap<PollEventSet, WasiSubscription>, tasks: Arc<dyn VirtualTaskManager + Send + Sync + 'static>) -> InodeValFilePollGuard {
        InodeValFilePollGuard {
            fd,
            subscriptions,
            mode: InodeValFilePollGuardMode::File(self.file),
            tasks,
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
            guard: unsafe { std::mem::transmute(guard) }
        }
    }
    pub(crate) fn swap(&mut self, mut file: Box<dyn VirtualFile + Send + Sync + 'static>) -> Box<dyn VirtualFile + Send + Sync + 'static> {
        std::mem::swap(self.guard.deref_mut(), &mut file);
        file
    }
}

impl<'a> Deref for InodeValFileWriteGuard {
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
    pub fn new(state: &WasiState, fd: __wasi_fd_t) -> Result<Option<Self>, FsError> {
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

    pub fn lock_read(
        &self,
        inodes: &RwLockReadGuard<WasiInodes>,
    ) -> Option<InodeValFileReadGuard> {
        let guard = inodes.arena[self.inode].read();
        if let Kind::File { handle, .. } = guard.deref() {
            if let Some(handle) = handle.as_ref() {
                Some(InodeValFileReadGuard::new(handle))
            } else {
                None
            }
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
            if let Some(handle) = handle.as_ref() {
                Some(InodeValFileWriteGuard::new(handle))
            } else {
                None
            }
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

    fn sync_to_disk(&self) -> Result<(), FsError> {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.as_ref() {
            file.sync_to_disk()
        } else {
            Err(FsError::IOError)
        }
    }

    fn bytes_available(&self) -> Result<usize, FsError> {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.as_ref() {
            file.bytes_available()
        } else {
            Err(FsError::IOError)
        }
    }

    fn bytes_available_read(&self) -> Result<Option<usize>, FsError> {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.as_ref() {
            file.bytes_available_read()
        } else {
            Err(FsError::IOError)
        }
    }

    fn bytes_available_write(&self) -> Result<Option<usize>, FsError> {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.as_ref() {
            file.bytes_available_write()
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

    fn get_fd(&self) -> Option<wasmer_vfs::FileDescriptor> {
        let inodes = self.inodes.read().unwrap();
        let guard = self.lock_read(&inodes);
        if let Some(file) = guard.as_ref() {
            file.get_fd()
        } else {
            None
        }
    }
}

impl Write for WasiStateFileGuard {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.as_mut () {
            file.write(buf)
        } else {
            Err(std::io::ErrorKind::Unsupported.into())
        }
    }

    fn write_vectored(&mut self, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.as_mut() {
            file.write_vectored(bufs)
        } else {
            Err(std::io::ErrorKind::Unsupported.into())
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.as_mut() {
            file.flush()
        } else {
            Err(std::io::ErrorKind::Unsupported.into())
        }
    }
}

impl Read for WasiStateFileGuard {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.as_mut() {
            file.read(buf)
        } else {
            Err(std::io::ErrorKind::Unsupported.into())
        }
    }

    fn read_vectored(&mut self, bufs: &mut [std::io::IoSliceMut<'_>]) -> std::io::Result<usize> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.as_mut() {
            file.read_vectored(bufs)
        } else {
            Err(std::io::ErrorKind::Unsupported.into())
        }
    }
}

impl Seek for WasiStateFileGuard {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        let inodes = self.inodes.read().unwrap();
        let mut guard = self.lock_write(&inodes);
        if let Some(file) = guard.as_mut() {
            file.seek(pos)
        } else {
            Err(std::io::ErrorKind::Unsupported.into())
        }
    }
}
