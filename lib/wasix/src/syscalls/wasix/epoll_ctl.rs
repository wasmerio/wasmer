use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc::UnboundedSender, watch};
use virtual_mio::{InterestHandler, InterestType};
use virtual_net::net_error_into_io_err;
use wasmer_wasix_types::wasi::{
    EpollCtl, EpollEvent, EpollEventCtl, EpollType, SubscriptionClock, SubscriptionUnion, Userdata,
};

use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use futures::Future;

use super::*;
use crate::{
    fs::{
        EpollFd, EpollInterest, EpollJoinGuard, InodeValFilePollGuard, InodeValFilePollGuardJoin,
        InodeValFilePollGuardMode, POLL_GUARD_MAX_RET,
    },
    state::PollEventSet,
    syscalls::*,
    WasiInodes,
};

/// ### `epoll_ctl()`
/// Modifies an epoll interest list
/// Output:
/// - `Fd fd`
///   The new file handle that is used to modify or wait on the interest list
#[instrument(level = "trace", skip_all, fields(timeout_ms = field::Empty, fd_guards = field::Empty, seen = field::Empty, fd), ret)]
pub fn epoll_ctl<M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    epfd: WasiFd,
    op: EpollCtl,
    fd: WasiFd,
    event_ref: WasmPtr<EpollEvent<M>, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();

    let memory = unsafe { env.memory_view(&ctx) };
    let event = if event_ref.offset() != M::ZERO {
        Some(wasi_try_mem_ok!(event_ref.read(&memory)))
    } else {
        None
    };

    let event_ctl = event.map(|evt| EpollEventCtl {
        events: evt.events,
        ptr: evt.data.ptr.into(),
        fd: evt.data.fd,
        data1: evt.data.data1,
        data2: evt.data.data2,
    });

    wasi_try_ok!(epoll_ctl_internal(
        &mut ctx,
        epfd,
        op,
        fd,
        event_ctl.as_ref()
    )?);
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_epoll_ctl(&mut ctx, epfd, op, fd, event_ctl).map_err(|err| {
            tracing::error!("failed to save epoll_create event - {}", err);
            WasiError::Exit(ExitCode::Errno(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn epoll_ctl_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    epfd: WasiFd,
    op: EpollCtl,
    fd: WasiFd,
    event_ctl: Option<&EpollEventCtl>,
) -> Result<Result<(), Errno>, WasiError> {
    let env = ctx.data();
    let fd_entry = wasi_try_ok_ok!(env.state.fs.get_fd(epfd));

    let tasks = env.tasks().clone();
    let mut inode_guard = fd_entry.inode.read();
    match inode_guard.deref() {
        Kind::Epoll {
            subscriptions, tx, ..
        } => {
            if let EpollCtl::Del | EpollCtl::Mod = op {
                let mut guard = subscriptions.lock().unwrap();
                guard.remove(&fd);

                tracing::trace!(fd, "unregistering waker");
            }
            if let EpollCtl::Add | EpollCtl::Mod = op {
                if let Some(event) = event_ctl {
                    let epoll_fd = EpollFd {
                        events: event.events,
                        ptr: event.ptr,
                        fd: event.fd,
                        data1: event.data1,
                        data2: event.data2,
                    };

                    // Output debug
                    tracing::trace!(
                        peb = ?event.events,
                        ptr = ?event.ptr,
                        data1 = event.data1,
                        data2 = event.data2,
                        fd = event.fd,
                        "registering waker"
                    );

                    {
                        // We have to register the subscription before we register the waker
                        // as otherwise there is a race condition
                        let mut guard = subscriptions.lock().unwrap();
                        guard.insert(event.fd, (epoll_fd.clone(), Vec::new()));
                    }

                    // Now we register the epoll waker
                    let tx = tx.clone();
                    let mut fd_guards =
                        wasi_try_ok_ok!(register_epoll_waker(&env.state, &epoll_fd, tx));

                    // After the guards are created we need to attach them to the subscription
                    let mut guard = subscriptions.lock().unwrap();
                    if let Some(subs) = guard.get_mut(&event.fd) {
                        subs.1.append(&mut fd_guards);
                    }
                }
            }
            Ok(Ok(()))
        }
        _ => Ok(Err(Errno::Inval)),
    }
}

#[derive(Debug)]
pub struct EpollJoinWaker {
    fd: WasiFd,
    readiness: EpollType,
    tx: Arc<watch::Sender<EpollInterest>>,
}
impl EpollJoinWaker {
    pub fn new(
        fd: WasiFd,
        readiness: EpollType,
        tx: Arc<watch::Sender<EpollInterest>>,
    ) -> Arc<Self> {
        Arc::new(Self { fd, readiness, tx })
    }

    fn wake_now(&self) {
        self.tx.send_modify(|i| {
            i.interest.insert((self.fd, self.readiness));
        });
    }
    pub fn as_waker(self: &Arc<Self>) -> Waker {
        let s: *const Self = Arc::into_raw(Arc::clone(self));
        let raw_waker = RawWaker::new(s as *const (), &VTABLE);
        unsafe { Waker::from_raw(raw_waker) }
    }
}
pub struct EpollHandler {
    fd: WasiFd,
    tx: Arc<watch::Sender<EpollInterest>>,
}
impl EpollHandler {
    pub fn new(fd: WasiFd, tx: Arc<watch::Sender<EpollInterest>>) -> Box<Self> {
        Box::new(Self { fd, tx })
    }
}
impl InterestHandler for EpollHandler {
    fn push_interest(&mut self, interest: InterestType) {
        let readiness = match interest {
            InterestType::Readable => EpollType::EPOLLIN,
            InterestType::Writable => EpollType::EPOLLOUT,
            InterestType::Closed => EpollType::EPOLLHUP,
            InterestType::Error => EpollType::EPOLLERR,
        };
        self.tx.send_modify(|i| {
            i.interest.insert((self.fd, readiness));
        });
    }

    fn pop_interest(&mut self, interest: InterestType) -> bool {
        let readiness = match interest {
            InterestType::Readable => EpollType::EPOLLIN,
            InterestType::Writable => EpollType::EPOLLOUT,
            InterestType::Closed => EpollType::EPOLLHUP,
            InterestType::Error => EpollType::EPOLLERR,
        };
        let mut ret = false;
        self.tx.send_modify(move |i| {
            ret = i.interest.iter().any(|(_, b)| *b == readiness);
            i.interest.retain(|(_, b)| *b != readiness);
        });
        ret
    }

    fn has_interest(&self, interest: InterestType) -> bool {
        let readiness = match interest {
            InterestType::Readable => EpollType::EPOLLIN,
            InterestType::Writable => EpollType::EPOLLOUT,
            InterestType::Closed => EpollType::EPOLLHUP,
            InterestType::Error => EpollType::EPOLLERR,
        };
        let mut ret = false;
        self.tx.send_modify(move |i| {
            ret = i.interest.iter().any(|(_, b)| *b == readiness);
        });
        ret
    }
}

fn inline_waker_wake(s: &EpollJoinWaker) {
    let waker_arc = unsafe { Arc::from_raw(s) };
    waker_arc.wake_now();
}

fn inline_waker_clone(s: &EpollJoinWaker) -> RawWaker {
    let arc = unsafe { Arc::from_raw(s) };
    std::mem::forget(arc.clone());
    RawWaker::new(Arc::into_raw(arc) as *const (), &VTABLE)
}

const VTABLE: RawWakerVTable = unsafe {
    RawWakerVTable::new(
        |s| inline_waker_clone(&*(s as *const EpollJoinWaker)), // clone
        |s| inline_waker_wake(&*(s as *const EpollJoinWaker)),  // wake
        |s| (*(s as *const EpollJoinWaker)).wake_now(), // wake by ref (don't decrease refcount)
        |s| drop(Arc::from_raw(s as *const EpollJoinWaker)), // decrease refcount
    )
};

pub(super) fn register_epoll_waker(
    state: &Arc<WasiState>,
    event: &EpollFd,
    tx: Arc<watch::Sender<EpollInterest>>,
) -> Result<Vec<EpollJoinGuard>, Errno> {
    let mut type_ = Eventtype::FdRead;
    let mut peb = PollEventBuilder::new();
    if event.events.contains(EpollType::EPOLLOUT) {
        type_ = Eventtype::FdWrite;
        peb = peb.add(PollEvent::PollOut);
    }
    if event.events.contains(EpollType::EPOLLIN) {
        type_ = Eventtype::FdRead;
        peb = peb.add(PollEvent::PollIn);
    }
    if event.events.contains(EpollType::EPOLLERR) {
        peb = peb.add(PollEvent::PollError);
    }
    if event.events.contains(EpollType::EPOLLHUP) | event.events.contains(EpollType::EPOLLRDHUP) {
        peb = peb.add(PollEvent::PollHangUp);
    }

    // Create a dummy subscription
    let s = Subscription {
        userdata: event.data2,
        type_,
        data: SubscriptionUnion {
            fd_readwrite: SubscriptionFsReadwrite {
                file_descriptor: event.fd,
            },
        },
    };

    // Get guard object which we will register the waker against
    let mut ret = Vec::new();
    let fd_guard = poll_fd_guard(state, peb.build(), event.fd, s)?;
    match &fd_guard.mode {
        // Sockets now use epoll
        InodeValFilePollGuardMode::Socket { inner, .. } => {
            let handler = EpollHandler::new(event.fd, tx);

            let mut inner = inner.protected.write().unwrap();
            inner.set_handler(handler).map_err(net_error_into_io_err)?;
            drop(inner);

            ret.push(EpollJoinGuard::Handler { fd_guard })
        }
        _ => {
            // Otherwise we fall back on the regular polling guard

            // First we create the waker
            let epoll_waker = EpollJoinWaker::new(event.fd, event.events, tx);
            let waker = epoll_waker.as_waker();
            let mut cx = Context::from_waker(&waker);

            // Now we use the waker to trigger events
            let mut join_guard = InodeValFilePollGuardJoin::new(fd_guard);
            if Pin::new(&mut join_guard).poll(&mut cx).is_ready() {
                waker.wake();
            }
            ret.push(EpollJoinGuard::Join {
                join_guard,
                epoll_waker,
            });
        }
    }
    Ok(ret)
}
