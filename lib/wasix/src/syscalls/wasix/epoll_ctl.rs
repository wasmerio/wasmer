use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;
use wasmer_wasix_types::wasi::{
    EpollCtl, EpollEvent, EpollType, SubscriptionClock, SubscriptionUnion, Userdata,
};

use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use futures::Future;

use super::*;
use crate::{
    fs::{EpollFd, InodeValFilePollGuard, InodeValFilePollGuardJoin},
    state::PollEventSet,
    syscalls::*,
    WasiInodes,
};

/// ### `epoll_ctl()`
/// Modifies an epoll interest list
/// Output:
/// - `Fd fd`
///   The new file handle that is used to modify or wait on the interest list
#[instrument(level = "trace", skip_all, fields(timeout_ms = field::Empty, fd_guards = field::Empty, seen = field::Empty, fd), ret, err)]
pub fn epoll_ctl<M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    epfd: WasiFd,
    op: EpollCtl,
    fd: WasiFd,
    event_ref: WasmPtr<EpollEvent<M>, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();

    let memory = unsafe { env.memory_view(&ctx) };
    let event = wasi_try_mem_ok!(event_ref.read(&memory));

    let fd_entry = wasi_try_ok!(env.state.fs.get_fd(epfd));

    let inode = fd_entry.inode.clone();
    let tasks = env.tasks().clone();
    let mut inode_guard = inode.read();
    match inode_guard.deref() {
        Kind::Epoll {
            subscriptions, tx, ..
        } => {
            if let EpollCtl::Del | EpollCtl::Mod = op {
                let mut guard = subscriptions.lock().unwrap();
                guard.remove(&(event.data.fd, event.events));

                tracing::trace!(fd = event.data.fd, "unregistering waker");
            }
            if let EpollCtl::Add | EpollCtl::Mod = op {
                let epoll_fd = EpollFd {
                    events: event.events,
                    ptr: wasi_try_ok!(event.data.ptr.try_into().map_err(|_| Errno::Overflow)),
                    fd: event.data.fd,
                    data1: event.data.data1,
                    data2: event.data.data2,
                };
                {
                    let mut guard = subscriptions.lock().unwrap();
                    guard.insert((event.data.fd, event.events), epoll_fd.clone());
                }

                // Now we create a waker that will send a notification if the
                // event we are listening is triggered for this FD
                let tx = tx.clone();
                drop(inode_guard);

                // Output debug
                tracing::trace!(
                    peb = ?event.events,
                    ptr = ?event.data.ptr,
                    data1 = event.data.data1,
                    data2 = event.data.data2,
                    fd = event.data.fd,
                    "registering waker"
                );

                // Now we register the epoll waker
                wasi_try_ok!(register_epoll_waker(env.state(), &epoll_fd, tx));
            }
            Ok(Errno::Success)
        }
        _ => Ok(Errno::Inval),
    }
}

pub struct EpollWaker {
    fd: WasiFd,
    readiness: EpollType,
    tx: Arc<UnboundedSender<(WasiFd, EpollType)>>,
}
impl EpollWaker {
    pub fn new(
        fd: WasiFd,
        readiness: EpollType,
        tx: Arc<UnboundedSender<(WasiFd, EpollType)>>,
    ) -> Arc<Self> {
        Arc::new(Self { fd, tx, readiness })
    }

    fn wake_now(&self) {
        self.tx.send((self.fd, self.readiness)).ok();
    }
    pub fn as_waker(self: &Arc<Self>) -> Waker {
        let s: *const Self = Arc::into_raw(Arc::clone(self));
        let raw_waker = RawWaker::new(s as *const (), &VTABLE);
        unsafe { Waker::from_raw(raw_waker) }
    }
}

fn inline_waker_wake(s: &EpollWaker) {
    let waker_arc = unsafe { Arc::from_raw(s) };
    waker_arc.wake_now();
}

fn inline_waker_clone(s: &EpollWaker) -> RawWaker {
    let arc = unsafe { Arc::from_raw(s) };
    std::mem::forget(arc.clone());
    RawWaker::new(Arc::into_raw(arc) as *const (), &VTABLE)
}

const VTABLE: RawWakerVTable = unsafe {
    RawWakerVTable::new(
        |s| inline_waker_clone(&*(s as *const EpollWaker)), // clone
        |s| inline_waker_wake(&*(s as *const EpollWaker)),  // wake
        |s| (*(s as *const EpollWaker)).wake_now(),         // wake by ref (don't decrease refcount)
        |s| drop(Arc::from_raw(s as *const EpollWaker)),    // decrease refcount
    )
};

pub(super) fn register_epoll_waker(
    state: &WasiState,
    event: &EpollFd,
    tx: Arc<UnboundedSender<(WasiFd, EpollType)>>,
) -> Result<bool, Errno> {
    // First we create the waker
    let waker = EpollWaker::new(event.fd, event.events, tx).as_waker();
    let mut cx = Context::from_waker(&waker);

    // Create a dummy subscription
    let s = Subscription {
        userdata: event.data2,
        type_: Eventtype::FdRead,
        data: SubscriptionUnion {
            fd_readwrite: SubscriptionFsReadwrite {
                file_descriptor: event.fd,
            },
        },
    };

    // Generate the peb
    let mut peb = PollEventBuilder::new();
    if event.events.contains(EpollType::EPOLLIN) {
        peb = peb.add(PollEvent::PollIn);
    }
    if event.events.contains(EpollType::EPOLLOUT) {
        peb = peb.add(PollEvent::PollOut);
    }
    if event.events.contains(EpollType::EPOLLERR) {
        peb = peb.add(PollEvent::PollError);
    }
    if event.events.contains(EpollType::EPOLLHUP) || event.events.contains(EpollType::EPOLLRDHUP) {
        peb = peb.add(PollEvent::PollHangUp);
    }

    // Get guard object which we will register the waker against
    let fd_guard = poll_fd_guard(state, peb.build(), event.fd, s)?;
    let mut fd_guard = InodeValFilePollGuardJoin::new(fd_guard);

    // Depending on the events we register the waker against the right polling operation
    if Pin::new(&mut fd_guard).poll(&mut cx).is_ready() {
        waker.wake();
        Ok(true)
    } else {
        Ok(false)
    }
}
