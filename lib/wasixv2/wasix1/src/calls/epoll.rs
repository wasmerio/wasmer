//! `epoll_create`, `epoll_ctl`, `epoll_wait` over Linux's epoll.
//!
//! v1's `epoll_data` (pointer, descriptor and two more words, 24 bytes) does not fit Linux's
//! 64-bit `epoll_data`, so the compat layer keeps the guest's data in a side table and the
//! kernel's item carries a handle to it. Events find their data by that handle alone: several
//! descriptors may name one epoll set (mio `dup`s its set and registers through the copy while
//! it waits on the original), and only the kernel knows which ones do.

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

/// A registration's handle, the kernel item's data: the watched descriptor in the high half
/// (so that an item the table does not know still reports its descriptor) and a counter in the
/// low half (so that a re-registration never reuses a live handle).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Handle(u64);

impl Handle {
    fn fd(self) -> Fd {
        Fd((self.0 >> 32) as u32 as i32)
    }
}

/// The guest's data of every registration, by handle.
///
/// The index by (epoll descriptor, watched descriptor) serves `EPOLL_CTL_MOD`/`DEL`, which
/// replace or drop the data registered before. Like the descriptors it is keyed by, a record
/// goes when its epoll descriptor is closed or comes to name another file (see [`forgotten`]). Every record belongs to exactly one index
/// entry, so the table holds at most one record per pair the guest has used, also for items
/// the kernel dropped on its own (the watched file's last close) or changed through another
/// descriptor of the set.
#[derive(Debug, Default)]
pub struct Table(Mutex<Registrations>);

#[derive(Clone, Debug, Default)]
struct Registrations {
    data: HashMap<Handle, EpollData>,
    index: HashMap<(Fd, Fd), Handle>,
    next: u32,
}

impl Table {
    /// A copy for a fork child, whose epoll sets carry the same handles.
    pub(crate) fn fork(&self) -> Self {
        Self(Mutex::new(self.0.lock().unwrap().clone()))
    }

    /// A fresh handle for `fd`.
    fn handle(&self, fd: Fd) -> Handle {
        let mut regs = self.0.lock().unwrap();
        regs.next = regs.next.wrapping_add(1);
        Handle(u64::from(fd.0 as u32) << 32 | u64::from(regs.next))
    }

    /// Records `data` under `handle`, which the kernel's item for (`epfd`, `fd`) now carries.
    fn set(&self, epfd: Fd, fd: Fd, handle: Handle, data: EpollData) {
        let mut regs = self.0.lock().unwrap();
        if let Some(old) = regs.index.insert((epfd, fd), handle) {
            regs.data.remove(&old);
        }
        regs.data.insert(handle, data);
    }

    fn remove(&self, epfd: Fd, fd: Fd) {
        let mut regs = self.0.lock().unwrap();
        if let Some(old) = regs.index.remove(&(epfd, fd)) {
            regs.data.remove(&old);
        }
    }

    /// Forgets everything registered through `epfd`, a descriptor that was closed or now names
    /// another file, such as a new epoll set.
    fn forget(&self, epfd: Fd) {
        let regs = &mut *self.0.lock().unwrap();
        regs.index.retain(|(set, _), handle| {
            let keep = *set != epfd;
            if !keep {
                regs.data.remove(handle);
            }
            keep
        });
    }

    /// The data registered under `handle`; just the descriptor if there is none (an item
    /// this process image did not register, such as one inherited across `execve`).
    fn get(&self, handle: Handle) -> EpollData {
        self.0
            .lock()
            .unwrap()
            .data
            .get(&handle)
            .copied()
            .unwrap_or(EpollData {
                fd: handle.fd().0 as u32,
                ..EpollData::ZERO
            })
    }
}

/// Forgets what the process kept about the descriptor `fd`, which was closed or now names
/// another file: its epoll registrations as a set, and its socket timeouts.
pub(crate) async fn forgotten<S: Syscalls + Control + 'static>(
    frame: &crate::shim::Frame<'_, S>,
    fd: Fd,
) -> crate::errno::V1Result<()> {
    let proc = frame.proc().await?;
    proc.net.epoll.forget(fd);
    proc.net.timeouts.forget(fd);
    Ok(())
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
        let table = &frame.proc().await?.net.epoll;
        let mut registered = None;
        let mut ptr = GuestPtr::NULL;
        if op != EpollCtlOp::EPOLL_CTL_DEL {
            let v1: EpollEvent = frame.read(event as u32).await?;
            let handle = table.handle(fd);
            let linux = LinuxEvent {
                events: to_linux(v1.events),
                data: LinuxData(handle.0.to_le_bytes()),
            };
            ptr = frame.mem.alloc(&linux)?;
            registered = Some((handle, v1.data));
        }
        frame
            .syscalls
            .epoll_ctl(&frame.caller(), epfd, op, fd, ptr)
            .await?;
        match registered {
            Some((handle, data)) => table.set(epfd, fd, handle, data),
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
            let event = EpollEvent {
                events: from_linux(linux.events),
                data: table.get(Handle(u64::from_le_bytes(linux.data.0))),
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

    fn data(n: u32) -> EpollData {
        EpollData {
            ptr: n,
            fd: 5,
            data1: n + 1,
            data2: u64::from(n) << 40,
        }
    }

    /// The data follows the handle, not the descriptor the set is used through.
    #[test]
    fn side_table() {
        let table = Table::default();
        let (set, dup, fd) = (Fd(4), Fd(8), Fd(5));
        let first = table.handle(fd);
        assert_eq!(first.fd(), fd);
        table.set(dup, fd, first, data(1));
        assert_eq!(table.get(first), data(1));

        // A MOD replaces the record: the old handle is gone, the new one has the new data.
        let second = table.handle(fd);
        assert_ne!(first, second);
        table.set(dup, fd, second, data(2));
        assert_eq!(table.get(second), data(2));
        assert_eq!(
            table.get(first),
            EpollData {
                fd: 5,
                ..EpollData::ZERO
            }
        );

        // An unknown handle still names its descriptor.
        let unknown = table.handle(Fd(9));
        assert_eq!(table.get(unknown).fd, 9);

        // Registrations through another descriptor are separate index entries.
        let third = table.handle(fd);
        table.set(set, fd, third, data(3));
        table.remove(dup, fd);
        assert_eq!(table.get(second).data2, 0);
        assert_eq!(table.get(third), data(3));
        table.forget(set);
        assert_eq!(table.get(third).data2, 0);
        let regs = table.0.lock().unwrap();
        assert!(regs.data.is_empty() && regs.index.is_empty());
    }
}
