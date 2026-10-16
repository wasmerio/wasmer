//! `poll_oneoff` over `ppoll_time64`.
//!
//! Descriptor subscriptions become `pollfd`s (one per descriptor, interests merged); clock
//! subscriptions become deadlines, the earliest of which bounds the wait. v1 quirks kept:
//! a clock with a timeout of 0 never fires (wasix-libc asks for 1 to mean "now"), and only
//! the realtime and monotonic clocks can be waited on.

use std::time::Duration;

use wasixv2_abi::{
    AtFlags, ClockId, Fd, GuestPtr, GuestPtrMut, IoctlRequest, KernelTimespec, PollEvents, Pollfd,
    Statx, StatxMask,
};
use wasixv2_syscalls::{Control, Syscalls};
use wasmer::RuntimeError;

use crate::{
    abi::{
        Clockid, Errno, GuestLayout,
        poll::{
            Event, EventFdReadwrite, Eventrwflags, Eventtype, Snapshot0Subscription, Subclockflags,
            Subscription,
        },
    },
    errno::V1Result,
    shim::{self, Env, Frame},
};

/// What one subscription waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wait {
    Read(u32),
    Write(u32),
    Clock {
        id: Clockid,
        timeout: u64,
        abstime: bool,
    },
    /// A tag v1 does not define, which never fires.
    Nothing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sub {
    userdata: u64,
    wait: Wait,
}

/// The subscription layouts of `poll_oneoff`: preview1's and `wasi_unstable`'s.
trait SubscriptionLayout: GuestLayout {
    fn sub(&self) -> Sub;
}

impl SubscriptionLayout for Subscription {
    fn sub(&self) -> Sub {
        let wait = match (self.tag, self.clock(), self.fd()) {
            (Eventtype::CLOCK, Some(c), _) => Wait::Clock {
                id: Clockid(c.id),
                timeout: c.timeout,
                abstime: c.flags.0 & Subclockflags::ABSTIME.0 != 0,
            },
            (Eventtype::FD_READ, _, Some(fd)) => Wait::Read(fd),
            (Eventtype::FD_WRITE, _, Some(fd)) => Wait::Write(fd),
            _ => Wait::Nothing,
        };
        Sub {
            userdata: self.userdata,
            wait,
        }
    }
}

impl SubscriptionLayout for Snapshot0Subscription {
    fn sub(&self) -> Sub {
        let wait = match (self.tag, self.clock(), self.fd()) {
            (Eventtype::CLOCK, Some(c), _) => Wait::Clock {
                id: Clockid(c.id),
                timeout: c.timeout,
                abstime: c.flags.0 & Subclockflags::ABSTIME.0 != 0,
            },
            (Eventtype::FD_READ, _, Some(fd)) => Wait::Read(fd),
            (Eventtype::FD_WRITE, _, Some(fd)) => Wait::Write(fd),
            _ => Wait::Nothing,
        };
        Sub {
            userdata: self.userdata,
            wait,
        }
    }
}

/// The `pollfd` set for `subs`: one entry per descriptor, and for each subscription the index
/// of its entry. Descriptors that cannot be open (beyond `i32::MAX`) get none.
fn pollfds(subs: &[Sub]) -> (Vec<Pollfd>, Vec<Option<usize>>) {
    let mut fds: Vec<Pollfd> = Vec::new();
    let mut index = Vec::with_capacity(subs.len());
    for sub in subs {
        let (fd, events) = match sub.wait {
            Wait::Read(fd) => (
                fd,
                PollEvents(PollEvents::POLLIN.0 | PollEvents::POLLRDHUP.0),
            ),
            Wait::Write(fd) => (fd, PollEvents::POLLOUT),
            _ => {
                index.push(None);
                continue;
            }
        };
        let Ok(fd) = i32::try_from(fd) else {
            index.push(None);
            continue;
        };
        let at = match fds.iter().position(|p| p.fd == Fd(fd)) {
            Some(at) => at,
            None => {
                fds.push(Pollfd {
                    fd: Fd(fd),
                    events: PollEvents(0),
                    revents: PollEvents(0),
                });
                fds.len() - 1
            }
        };
        fds[at].events.0 |= events.0;
        index.push(Some(at));
    }
    (fds, index)
}

/// The revents a subscription reports on.
fn interest(wait: Wait) -> u16 {
    let common = PollEvents::POLLHUP.0 | PollEvents::POLLERR.0 | PollEvents::POLLNVAL.0;
    match wait {
        Wait::Read(_) => common | PollEvents::POLLIN.0 | PollEvents::POLLRDHUP.0,
        Wait::Write(_) => common | PollEvents::POLLOUT.0,
        _ => 0,
    }
}

fn timespec(d: Duration) -> KernelTimespec {
    KernelTimespec {
        tv_sec: d.as_secs().min(i64::MAX as u64) as i64,
        tv_nsec: i64::from(d.subsec_nanos()),
    }
}

/// `ppoll` over `fds` for at most `timeout` (forever when `None`); returns the ready count
/// and leaves `revents` in `fds`.
async fn ppoll<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fds: &mut [Pollfd],
    timeout: Option<Duration>,
) -> V1Result<i32> {
    let mut array = GuestPtrMut::<Pollfd>::NULL;
    if !fds.is_empty() {
        let addr = frame.mem.alloc_raw((fds.len() * Pollfd::SIZE) as u32, 8)?;
        for (i, fd) in fds.iter().enumerate() {
            let ptr = GuestPtrMut::<Pollfd>::new(addr + (i * Pollfd::SIZE) as u32);
            wasixv2_syscalls::write_value(&frame.mem, ptr, fd).await?;
        }
        array = GuestPtrMut::new(addr);
    }
    let tsp = match timeout {
        Some(timeout) => GuestPtrMut::new(frame.mem.alloc(&timespec(timeout))?.addr()),
        None => GuestPtrMut::NULL,
    };
    let ready = frame
        .syscalls
        .ppoll_time64(
            &frame.caller(),
            array,
            fds.len() as u32,
            tsp,
            GuestPtr::NULL,
            8,
        )
        .await?;
    for (i, fd) in fds.iter_mut().enumerate() {
        *fd = frame
            .mem
            .take(GuestPtrMut::new(array.addr() + (i * Pollfd::SIZE) as u32));
    }
    Ok(ready)
}

/// Waits until `fd` has one of `events` or `timeout` passes; the revents, empty on timeout.
pub(crate) async fn wait_fd<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
    events: PollEvents,
    timeout: Option<Duration>,
) -> V1Result<PollEvents> {
    let mut fds = [Pollfd {
        fd,
        events,
        revents: PollEvents(0),
    }];
    ppoll(frame, &mut fds, timeout).await?;
    Ok(fds[0].revents)
}

/// The current time of `clock` in nanoseconds.
pub(crate) async fn now<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    clock: ClockId,
) -> V1Result<u64> {
    let ts = frame.mem.alloc_mut::<KernelTimespec>()?;
    frame
        .syscalls
        .clock_gettime64(&frame.caller(), clock, ts)
        .await?;
    let ts = frame.mem.take(ts);
    Ok((ts.tv_sec.max(0) as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(ts.tv_nsec.max(0) as u64))
}

/// A clock subscription's deadline: on `clock`, at `at` nanoseconds.
#[derive(Clone, Copy, Debug)]
struct Deadline {
    clock: ClockId,
    at: u64,
}

/// The deadline of a clock subscription, `None` for one that never fires (timeout 0).
async fn deadline<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    id: Clockid,
    timeout: u64,
    abstime: bool,
) -> V1Result<Option<Deadline>> {
    let clock = match id {
        Clockid::REALTIME | Clockid::MONOTONIC => id.to_linux().expect("a v1 clock"),
        _ => return shim::fail(Errno::INVAL),
    };
    if timeout == 0 {
        return Ok(None);
    }
    let at = if abstime {
        timeout
    } else {
        now(frame, clock).await?.saturating_add(timeout)
    };
    Ok(Some(Deadline { clock, at }))
}

/// `nbytes` of a ready descriptor: what `FIONREAD` says (what is left of a regular file,
/// what is queued on a pipe or socket) for reading, and for writing what is left of a
/// regular file. Best effort; 0 when Linux has no answer.
async fn nbytes<S: Syscalls + Control + 'static>(frame: &Frame<'_, S>, wait: Wait, fd: Fd) -> u64 {
    if matches!(wait, Wait::Write(_)) {
        let Ok(stat) = frame.mem.alloc_mut::<Statx>() else {
            return 0;
        };
        let Ok(empty) = frame.mem.alloc_cstr(b"") else {
            return 0;
        };
        let regular = frame
            .syscalls
            .statx(
                &frame.caller(),
                fd,
                empty,
                AtFlags::AT_EMPTY_PATH,
                StatxMask::STATX_TYPE,
                stat,
            )
            .await
            .is_ok()
            && u32::from(frame.mem.take(stat).stx_mode) & 0o170000 == 0o100000;
        if !regular {
            return 0;
        }
    }
    let Ok(count) = frame.mem.alloc_mut::<i32>() else {
        return 0;
    };
    match frame
        .syscalls
        .ioctl(&frame.caller(), fd, IoctlRequest::FIONREAD, count.addr())
        .await
    {
        Ok(_) => frame.mem.take(count).max(0) as u64,
        Err(_) => 0,
    }
}

/// The event of a descriptor subscription whose descriptor reported `revents`, if any.
async fn fd_event<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    sub: &Sub,
    fd: Fd,
    revents: PollEvents,
) -> Option<Event> {
    let ready = revents.0 & interest(sub.wait);
    if ready == 0 {
        return None;
    }
    let (type_, hangup) = match sub.wait {
        Wait::Read(_) => (
            Eventtype::FD_READ,
            PollEvents::POLLHUP.0 | PollEvents::POLLRDHUP.0,
        ),
        _ => (Eventtype::FD_WRITE, PollEvents::POLLHUP.0),
    };
    let mut event = Event {
        userdata: sub.userdata,
        error: Errno::SUCCESS,
        type_,
        fd_readwrite: EventFdReadwrite::ZERO,
    };
    if ready & PollEvents::POLLNVAL.0 != 0 {
        event.error = Errno::BADF;
    } else if ready & PollEvents::POLLERR.0 != 0 {
        // Not the socket's pending error: reading `SO_ERROR` would clear it, and Linux leaves
        // it for the next call on the socket. wasix-libc's `poll` turns any error into
        // `POLLERR`.
        event.error = Errno::IO;
    } else {
        event.fd_readwrite.nbytes = nbytes(frame, sub.wait, fd).await;
    }
    if ready & hangup != 0 {
        event.fd_readwrite.flags = Eventrwflags::FD_READWRITE_HANGUP;
    }
    Some(event)
}

fn clock_event(sub: &Sub) -> Event {
    Event {
        userdata: sub.userdata,
        error: Errno::SUCCESS,
        type_: Eventtype::CLOCK,
        fd_readwrite: EventFdReadwrite::ZERO,
    }
}

/// One `poll_oneoff` over subscriptions of layout `L`.
async fn poll_oneoff<S, L>(
    env: Env<S>,
    subs_ptr: i32,
    out: i32,
    nsubs: i32,
    nevents: i32,
) -> Result<i32, RuntimeError>
where
    S: Syscalls + Control + 'static,
    L: SubscriptionLayout,
{
    shim::errno(&env, async |frame| {
        let nsubs = nsubs as u32;
        if nsubs == 0 {
            return shim::fail(Errno::INVAL);
        }
        let mut subs = Vec::new();
        for i in 0..nsubs {
            let addr = (subs_ptr as u32).wrapping_add(i * L::SIZE as u32);
            subs.push(frame.read::<L>(addr).await?.sub());
        }
        let mut deadlines = Vec::with_capacity(subs.len());
        for sub in &subs {
            deadlines.push(match sub.wait {
                Wait::Clock {
                    id,
                    timeout,
                    abstime,
                } => deadline(frame, id, timeout, abstime).await?,
                _ => None,
            });
        }
        let (mut fds, index) = pollfds(&subs);
        // A descriptor that cannot exist is reported at once, as `poll` reports `POLLNVAL`.
        let invalid = subs
            .iter()
            .zip(&index)
            .any(|(sub, at)| matches!(sub.wait, Wait::Read(_) | Wait::Write(_)) && at.is_none());

        let mut timeout = None::<Duration>;
        let mut first = None::<usize>;
        for (i, deadline) in deadlines.iter().enumerate() {
            let Some(deadline) = deadline else { continue };
            let left = Duration::from_nanos(
                deadline
                    .at
                    .saturating_sub(now(frame, deadline.clock).await?),
            );
            if timeout.is_none_or(|t| left < t) {
                timeout = Some(left);
                first = Some(i);
            }
        }
        if invalid {
            timeout = Some(Duration::ZERO);
        }
        let ready = ppoll(frame, &mut fds, timeout).await?;

        let mut events = Vec::new();
        for (i, sub) in subs.iter().enumerate() {
            match sub.wait {
                Wait::Read(fd) | Wait::Write(fd) => {
                    let revents = match index[i] {
                        Some(at) => fds[at].revents,
                        None => PollEvents::POLLNVAL,
                    };
                    if let Some(event) = fd_event(frame, sub, Fd(fd as i32), revents).await {
                        events.push(event);
                    }
                }
                Wait::Clock { .. } => {
                    if let Some(deadline) = deadlines[i]
                        && now(frame, deadline.clock).await? >= deadline.at
                    {
                        events.push(clock_event(sub));
                    }
                }
                Wait::Nothing => {}
            }
        }
        // `ppoll` timed out on the earliest deadline; report it even if its clock (realtime,
        // which may be set) does not show it passed yet.
        if events.is_empty()
            && ready == 0
            && let Some(first) = first
        {
            events.push(clock_event(&subs[first]));
        }
        for (i, event) in events.iter().enumerate() {
            let addr = (out as u32).wrapping_add((i * Event::SIZE) as u32);
            frame.write(addr, event).await?;
        }
        frame.write(nevents as u32, &(events.len() as u32)).await
    })
    .await
}

pub(crate) async fn poll_oneoff_preview1<S: Syscalls + Control + 'static>(
    env: Env<S>,
    subs: i32,
    out: i32,
    nsubs: i32,
    nevents: i32,
) -> Result<i32, RuntimeError> {
    poll_oneoff::<S, Subscription>(env, subs, out, nsubs, nevents).await
}

/// `wasi_unstable`'s, whose clock subscriptions carry an identifier of their own.
pub(crate) async fn poll_oneoff_snapshot0<S: Syscalls + Control + 'static>(
    env: Env<S>,
    subs: i32,
    out: i32,
    nsubs: i32,
    nevents: i32,
) -> Result<i32, RuntimeError> {
    poll_oneoff::<S, Snapshot0Subscription>(env, subs, out, nsubs, nevents).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(wait: Wait) -> Sub {
        Sub { userdata: 0, wait }
    }

    #[test]
    fn subscriptions_merge_per_descriptor() {
        let subs = [
            sub(Wait::Read(3)),
            sub(Wait::Write(4)),
            sub(Wait::Write(3)),
            sub(Wait::Clock {
                id: Clockid::MONOTONIC,
                timeout: 1,
                abstime: false,
            }),
            sub(Wait::Read(u32::MAX)),
        ];
        let (fds, index) = pollfds(&subs);
        assert_eq!(fds.len(), 2);
        assert_eq!(fds[0].fd, Fd(3));
        assert_eq!(
            fds[0].events.0,
            PollEvents::POLLIN.0 | PollEvents::POLLRDHUP.0 | PollEvents::POLLOUT.0
        );
        assert_eq!(fds[1].events, PollEvents::POLLOUT);
        assert_eq!(index, [Some(0), Some(1), Some(0), None, None]);
    }

    #[test]
    fn interests() {
        assert_eq!(interest(Wait::Read(0)) & PollEvents::POLLOUT.0, 0);
        assert_ne!(interest(Wait::Read(0)) & PollEvents::POLLRDHUP.0, 0);
        assert_eq!(interest(Wait::Write(0)) & PollEvents::POLLIN.0, 0);
        assert_ne!(interest(Wait::Write(0)) & PollEvents::POLLHUP.0, 0);
    }

    #[test]
    fn decodes_both_layouts() {
        let mut bytes = [0u8; Subscription::SIZE];
        bytes[..8].copy_from_slice(&7u64.to_le_bytes());
        bytes[8] = Eventtype::CLOCK.0;
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[24..32].copy_from_slice(&5u64.to_le_bytes());
        bytes[40..42].copy_from_slice(&1u16.to_le_bytes());
        let s = Subscription::read_from(&bytes).unwrap().sub();
        assert_eq!(
            s,
            Sub {
                userdata: 7,
                wait: Wait::Clock {
                    id: Clockid::MONOTONIC,
                    timeout: 5,
                    abstime: true
                }
            }
        );
        let mut bytes = [0u8; Snapshot0Subscription::SIZE];
        bytes[8] = Eventtype::FD_WRITE.0;
        bytes[16..20].copy_from_slice(&9u32.to_le_bytes());
        let s = Snapshot0Subscription::read_from(&bytes).unwrap().sub();
        assert_eq!(s.wait, Wait::Write(9));
    }
}
