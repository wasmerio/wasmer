//! Protection against a stale, back-to-back double `fd_close` of the same
//! descriptor number racing with descriptor reuse on another thread.
//!
//! Some guest toolchains close a socket twice when the owning object is
//! dropped (the wrapper closes the raw descriptor and then the owned
//! descriptor inside it closes it again). On its own the second close is
//! harmless: the number is no longer open and `fd_close` reports `EBADF`.
//! In a multi-threaded guest, however, another thread may have allocated the
//! very same number in between (POSIX hands out the lowest free number), and
//! the second close then silently destroys *that* thread's descriptor. The
//! victim later sees `EBADF` on a file it just opened, `EBADF` from `accept`
//! on its listener, or `EEXIST` from `epoll_ctl` because the descriptor number
//! it was handed still carries a foreign epoll registration.
//!
//! The guard keeps, per host thread, the last descriptor the guest thread
//! running on it closed together with the allocation generation of that slot.
//! A close of the same number by the same guest thread is treated as stale
//! and answered with `EBADF` (without touching the table) when all of the
//! following hold:
//!
//! - the slot has been re-allocated since (its generation changed),
//! - the thread has not allocated any descriptor since its previous close,
//! - the thread has not looked the number up since its previous close.
//!
//! Any lookup of the number (read, write, `fcntl`, ...) or any allocation on
//! the same thread forgets the record, so a thread that legitimately receives
//! the reused number from another thread and uses it before closing it is not
//! affected. Only a thread that closes a number twice in a row, without ever
//! touching it in between, is denied the second close, which is exactly the
//! observable behaviour POSIX prescribes for closing a descriptor that is not
//! open.
//!
//! The refusal also forgets the record: the pattern being guarded against is
//! exactly two closes, so a duplicate is refused at most once and a later close
//! of the same number by the same thread (which may own it legitimately by
//! then, e.g. an object dropped on that thread) proceeds normally. This bounds
//! any misjudgement to a single descriptor operation.

use std::cell::Cell;

use wasmer_wasix_types::wasi::Fd as WasiFd;

/// The last descriptor closed by the guest thread running on this host thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LastClosedFd {
    /// Identity of the [`WasiFs`](super::WasiFs) the descriptor belonged to.
    pub(crate) fs_id: u64,
    /// Guest thread that performed the close.
    pub(crate) tid: u32,
    /// The closed descriptor number.
    pub(crate) fd: WasiFd,
    /// Allocation generation of the slot at the time it was closed.
    pub(crate) generation: u64,
}

thread_local! {
    static LAST_CLOSED: Cell<Option<LastClosedFd>> = const { Cell::new(None) };
}

/// Remembers that `fd` (slot generation `generation`) was just closed by
/// guest thread `tid` on the current host thread.
pub(crate) fn record_close(fs_id: u64, tid: u32, fd: WasiFd, generation: u64) {
    LAST_CLOSED.set(Some(LastClosedFd {
        fs_id,
        tid,
        fd,
        generation,
    }));
}

/// Returns the last close performed on the current host thread, if any.
pub(crate) fn last_closed() -> Option<LastClosedFd> {
    LAST_CLOSED.get()
}

/// Forgets the last close on the current host thread.
pub(crate) fn forget() {
    LAST_CLOSED.set(None);
}

/// Forgets the last close on the current host thread if it concerned `fd`.
///
/// Called whenever a descriptor number is looked up; a thread that uses a
/// number after closing it must have acquired it again legitimately.
#[inline]
pub(crate) fn note_lookup(fd: WasiFd) {
    if let Some(last) = LAST_CLOSED.get()
        && last.fd == fd
    {
        LAST_CLOSED.set(None);
    }
}

/// Forgets the last close on the current host thread.
///
/// Called whenever a descriptor is allocated; a thread that allocated a
/// descriptor after its last close may legitimately have received the same
/// number back.
#[inline]
pub(crate) fn note_allocation() {
    if LAST_CLOSED.get().is_some() {
        LAST_CLOSED.set(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_of_other_fd_keeps_record() {
        forget();
        record_close(1, 1, 7, 42);
        note_lookup(8);
        assert_eq!(
            last_closed(),
            Some(LastClosedFd {
                fs_id: 1,
                tid: 1,
                fd: 7,
                generation: 42
            })
        );
        note_lookup(7);
        assert_eq!(last_closed(), None);
    }

    #[test]
    fn allocation_forgets_record() {
        forget();
        record_close(1, 1, 7, 42);
        note_allocation();
        assert_eq!(last_closed(), None);
    }
}
