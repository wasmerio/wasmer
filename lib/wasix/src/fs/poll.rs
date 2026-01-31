
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use virtual_mio::{InterestHandler, InterestType};
use wasmer_wasix_types::wasi::{Errno, Event, EventFdReadwrite, Eventtype, Subscription, Userdata};

use crate::state::PollEventSet;

use super::fd_table::Kind;
use super::notification::NotificationInner;
use super::pipes::{DuplexPipe, PipeRx, PipeTx};
use super::stdio::Stdio;

pub const POLL_GUARD_MAX_RET: usize = 8;

#[derive(Debug)]
pub enum InodeValFilePollGuardMode {
    Vfs,
    Socket {
        inner: crate::net::socket::InodeSocket,
    },
    EventNotifications(Arc<NotificationInner>),
    DuplexPipe {
        pipe: Arc<DuplexPipe>,
    },
    PipeRx {
        rx: Arc<PipeRx>,
    },
    PipeTx {
        tx: Arc<PipeTx>,
    },
    Stdio {
        stdio: Arc<Stdio>,
    },
}

#[derive(Debug)]
pub struct InodeValFilePollGuard {
    fd: wasmer_wasix_types::wasi::Fd,
    peb: PollEventSet,
    sub: Subscription,
    pub mode: InodeValFilePollGuardMode,
}

impl InodeValFilePollGuard {
    pub fn new(
        fd: wasmer_wasix_types::wasi::Fd,
        peb: PollEventSet,
        sub: Subscription,
        kind: &Kind,
    ) -> Option<Self> {
        let mode = match kind {
            Kind::VfsFile { .. } | Kind::VfsDir { .. } => InodeValFilePollGuardMode::Vfs,
            Kind::Socket { socket } => InodeValFilePollGuardMode::Socket {
                inner: socket.clone(),
            },
            Kind::EventNotifications { inner } => {
                InodeValFilePollGuardMode::EventNotifications(inner.clone())
            }
            Kind::DuplexPipe { pipe } => {
                InodeValFilePollGuardMode::DuplexPipe { pipe: pipe.clone() }
            }
            Kind::PipeRx { rx } => InodeValFilePollGuardMode::PipeRx { rx: rx.clone() },
            Kind::PipeTx { tx } => InodeValFilePollGuardMode::PipeTx { tx: tx.clone() },
            Kind::Stdin { handle } | Kind::Stdout { handle } | Kind::Stderr { handle } => {
                InodeValFilePollGuardMode::Stdio {
                    stdio: handle.clone(),
                }
            }
            _ => return None,
        };
        Some(Self { fd, peb, sub, mode })
    }

    pub fn fd(&self) -> wasmer_wasix_types::wasi::Fd {
        self.fd
    }

    pub fn peb(&self) -> PollEventSet {
        self.peb
    }

    pub fn cleanup(&mut self) {
        match &mut self.mode {
            InodeValFilePollGuardMode::Socket { inner } => {
                let mut inner = inner.inner.protected.write().unwrap();
                inner.remove_handler();
            }
            InodeValFilePollGuardMode::EventNotifications(inner) => {
                inner.remove_interest_handler();
            }
            InodeValFilePollGuardMode::DuplexPipe { pipe } => {
                pipe.remove_interest_handler();
            }
            InodeValFilePollGuardMode::PipeRx { rx } => {
                rx.remove_interest_handler();
            }
            InodeValFilePollGuardMode::PipeTx { tx } => {
                tx.remove_interest_handler();
            }
            InodeValFilePollGuardMode::Stdio { stdio } => {
                stdio.remove_interest_handler();
            }
            _ => {}
        }
    }
}

#[derive(Debug)]
pub struct InodeValFilePollGuardJoin {
    guard: InodeValFilePollGuard,
}

impl InodeValFilePollGuardJoin {
    pub fn new(guard: InodeValFilePollGuard) -> Self {
        Self { guard }
    }

    pub fn fd(&self) -> wasmer_wasix_types::wasi::Fd {
        self.guard.fd
    }

    pub fn peb(&self) -> PollEventSet {
        self.guard.peb
    }
}

impl Future for InodeValFilePollGuardJoin {
    type Output = Vec<(Event, PollEventSet)>;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut events = Vec::new();
        let userdata = self.guard.sub.userdata;
        let readiness = self.guard.peb;

        // Minimal readiness: assume requested events are ready.
        let event = Event {
            userdata,
            error: Errno::Success,
            type_: match self.guard.sub.type_ {
                Eventtype::FdRead | Eventtype::FdWrite => self.guard.sub.type_,
                _ => Eventtype::FdRead,
            },
            u: wasmer_wasix_types::wasi::EventUnion {
                fd_readwrite: EventFdReadwrite {
                    nbytes: 0,
                    flags: 0,
                },
            },
        };
        events.push((event, readiness));
        Poll::Ready(events)
    }
}

pub struct EpollHandler {
    fd: wasmer_wasix_types::wasi::Fd,
    tx: Arc<tokio::sync::watch::Sender<super::fd_table::EpollInterest>>,
}

impl EpollHandler {
    pub fn new(
        fd: wasmer_wasix_types::wasi::Fd,
        tx: Arc<tokio::sync::watch::Sender<super::fd_table::EpollInterest>>,
    ) -> Box<Self> {
        Box::new(Self { fd, tx })
    }
}

impl InterestHandler for EpollHandler {
    fn push_interest(&mut self, interest: InterestType) {
        let readiness = match interest {
            InterestType::Readable => wasmer_wasix_types::wasi::EpollType::EPOLLIN,
            InterestType::Writable => wasmer_wasix_types::wasi::EpollType::EPOLLOUT,
            InterestType::Closed => wasmer_wasix_types::wasi::EpollType::EPOLLHUP,
            InterestType::Error => wasmer_wasix_types::wasi::EpollType::EPOLLERR,
        };
        self.tx.send_modify(|i| {
            i.interest.insert((self.fd, readiness));
        });
    }

    fn pop_interest(&mut self, _interest: InterestType) -> bool {
        false
    }

    fn has_interest(&self, _interest: InterestType) -> bool {
        false
    }
}
