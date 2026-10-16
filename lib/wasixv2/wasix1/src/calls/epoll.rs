//! `epoll_create`, `epoll_ctl`, `epoll_wait` over Linux's epoll.
//!
//! v1's `epoll_data` (pointer, descriptor and two more words, 24 bytes) does not fit Linux's
//! 64-bit `epoll_data`, so the kernel's item carries the watched descriptor, which is unique
//! within an epoll set, and the compat layer keeps the guest's data in a side table.

use std::{collections::HashMap, sync::Mutex, time::Duration};

use wasixv2_abi::{
    EpollCreateFlags, EpollCtlOp, EpollData as LinuxData, EpollEvent as LinuxEvent, EpollEvents,
    Fd, GuestLayout, GuestPtr, GuestPtrMut, KernelTimespec,
};
use wasixv2_syscalls::{Control, Syscalls};
use wasmer::RuntimeError;

use crate::{
    abi::{
        Errno,
        sock::{EpollCtl, EpollData, EpollEvent, EpollType},
    },
    shim::{self, Env},
};

/// v1 event bits and their Linux counterparts.
const BITS: [(EpollType, EpollEvents); 8] = [
    (EpollType::EPOLLIN, EpollEvents::EPOLLIN),
    (EpollType::EPOLLOUT, EpollEvents::EPOLLOUT),
    (EpollType::EPOLLRDHUP, EpollEvents::EPOLLRDHUP),
    (EpollType::EPOLLPRI, EpollEvents::EPOLLPRI),
    (EpollType::EPOLLERR, EpollEvents::EPOLLERR),
    (EpollType::EPOLLHUP, EpollEvents::EPOLLHUP),
    (EpollType::EPOLLET, EpollEvents::EPOLLET),
    (EpollType::EPOLLONESHOT, EpollEvents::EPOLLONESHOT),
];

fn to_linux(events: EpollType) -> EpollEvents {
    let mut out = 0;
    for (v1, linux) in BITS {
        if events.0 & v1.0 != 0 {
            out |= linux.0;
        }
    }
    EpollEvents(out)
}

fn from_linux(events: EpollEvents) -> EpollType {
    let mut out = 0;
    for (v1, linux) in BITS {
        if events.0 & linux.0 != 0 {
            out |= v1.0;
        }
    }
    EpollType(out)
}

/// The guest's data of every watched descriptor, by (epoll set, descriptor).
#[derive(Debug, Default)]
pub struct Table(Mutex<HashMap<(Fd, Fd), EpollData>>);

impl Table {
    fn set(&self, epfd: Fd, fd: Fd, data: EpollData) {
        self.0.lock().unwrap().insert((epfd, fd), data);
    }

    fn remove(&self, epfd: Fd, fd: Fd) {
        self.0.lock().unwrap().remove(&(epfd, fd));
    }

    /// Forgets everything about `epfd`, a descriptor that now names a new epoll set.
    fn forget(&self, epfd: Fd) {
        self.0.lock().unwrap().retain(|(set, _), _| *set != epfd);
    }

    /// The data registered for `fd`; just the descriptor if there is none (an item added
    /// before the set's descriptor was renumbered).
    fn get(&self, epfd: Fd, fd: Fd) -> EpollData {
        self.0
            .lock()
            .unwrap()
            .get(&(epfd, fd))
            .copied()
            .unwrap_or(EpollData {
                fd: fd.0 as u32,
                ..EpollData::ZERO
            })
    }
}

pub(crate) async fn epoll_create<S: Syscalls + Control + 'static>(
    env: Env<S>,
    ro_fd: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let fd = frame
            .syscalls
            .epoll_create1(&frame.caller(), EpollCreateFlags(0))
            .await?;
        frame.proc().await?.net.epoll.forget(fd);
        frame.write(ro_fd as u32, &(fd.0 as u32)).await
    })
    .await
}

pub(crate) async fn epoll_ctl<S: Syscalls + Control + 'static>(
    env: Env<S>,
    epfd: i32,
    op: i32,
    fd: i32,
    event: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let (epfd, fd) = (Fd(epfd), Fd(fd));
        let op = match EpollCtl(op as u32) {
            EpollCtl::ADD | EpollCtl::ADD_WITX => EpollCtlOp::EPOLL_CTL_ADD,
            EpollCtl::MOD => EpollCtlOp::EPOLL_CTL_MOD,
            EpollCtl::DEL => EpollCtlOp::EPOLL_CTL_DEL,
            _ => return shim::fail(Errno::INVAL),
        };
        let mut registered = None;
        let mut ptr = GuestPtr::NULL;
        if op != EpollCtlOp::EPOLL_CTL_DEL {
            let v1: EpollEvent = frame.read(event as u32).await?;
            let linux = LinuxEvent {
                events: to_linux(v1.events),
                data: LinuxData((fd.0 as u32 as u64).to_le_bytes()),
            };
            ptr = frame.mem.alloc(&linux)?;
            registered = Some(v1.data);
        }
        frame
            .syscalls
            .epoll_ctl(&frame.caller(), epfd, op, fd, ptr)
            .await?;
        let table = &frame.proc().await?.net.epoll;
        match registered {
            Some(data) => table.set(epfd, fd, data),
            None => table.remove(epfd, fd),
        }
        Ok(())
    })
    .await
}

/// `epoll_wait(epfd, events, maxevents, timeout, ro_nevents)`; `timeout` is in nanoseconds,
/// `u64::MAX` waits forever.
pub(crate) async fn epoll_wait<S: Syscalls + Control + 'static>(
    env: Env<S>,
    epfd: i32,
    events: i32,
    maxevents: i32,
    timeout: i64,
    ro_nevents: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let epfd = Fd(epfd);
        if maxevents <= 0 {
            return shim::fail(Errno::INVAL);
        }
        let buf = frame
            .mem
            .alloc_raw(maxevents as u32 * LinuxEvent::SIZE as u32, 8)?;
        let tsp = match timeout as u64 {
            u64::MAX => GuestPtr::NULL,
            ns => {
                let d = Duration::from_nanos(ns);
                let ts = KernelTimespec {
                    tv_sec: d.as_secs() as i64,
                    tv_nsec: i64::from(d.subsec_nanos()),
                };
                frame.mem.alloc(&ts)?
            }
        };
        let n = frame
            .syscalls
            .epoll_pwait2(
                &frame.caller(),
                epfd,
                GuestPtrMut::new(buf),
                maxevents,
                tsp,
                GuestPtr::NULL,
                8,
            )
            .await?;
        let table = &frame.proc().await?.net.epoll;
        for i in 0..n.max(0) as u32 {
            let linux: LinuxEvent = frame
                .mem
                .take(GuestPtrMut::new(buf + i * LinuxEvent::SIZE as u32));
            let fd = Fd(u64::from_le_bytes(linux.data.0) as u32 as i32);
            let event = EpollEvent {
                events: from_linux(linux.events),
                data: table.get(epfd, fd),
            };
            let addr = (events as u32).wrapping_add(i * EpollEvent::SIZE as u32);
            frame.write(addr, &event).await?;
        }
        frame.write(ro_nevents as u32, &(n.max(0) as u32)).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every v1 bit has its own Linux bit, and the translation is explicit both ways.
    #[test]
    fn event_bits_round_trip() {
        for (v1, linux) in BITS {
            assert_eq!(to_linux(v1), linux);
            assert_eq!(from_linux(linux), v1);
        }
        let all = EpollType(BITS.iter().fold(0, |acc, (v1, _)| acc | v1.0));
        assert_eq!(from_linux(to_linux(all)), all);
        assert_eq!(from_linux(EpollEvents::EPOLLNVAL), EpollType(0));
        // v1 numbers its bits differently from Linux.
        assert_ne!(EpollType::EPOLLRDHUP.0, EpollEvents::EPOLLRDHUP.0);
    }

    #[test]
    fn side_table() {
        let table = Table::default();
        let data = EpollData {
            ptr: 1,
            fd: 5,
            data1: 2,
            data2: 3,
        };
        table.set(Fd(4), Fd(5), data);
        assert_eq!(table.get(Fd(4), Fd(5)), data);
        assert_eq!(table.get(Fd(9), Fd(5)).fd, 5);
        table.forget(Fd(4));
        assert_eq!(table.get(Fd(4), Fd(5)).data2, 0);
    }
}
