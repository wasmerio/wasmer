//! A very simple data structure for holding the FDs a WASI process is using.
//! Keeps track of the first unused (i.e. freed) FD, which is slightly faster
//! than doing a linear search of the entire array each time.
//! Note, The Unix spec requires newly allocated FDs to always be the
//! lowest-numbered FD available.
//!
//! Mutating methods (`insert`, `remove`, `insert_first_free`, etc.) update
//! `InodeGuard` handle counts via `acquire_handle` / `drop_one_handle`. Callers
//! must hold `WasiFs::fd_map` write-locked for the duration of any sequence that
//! must be atomic with respect to concurrent open/close. Lock order: fd map before
//! inode (see module comment in `mod.rs`).

use super::{
    VirtualFileLock,
    fd::{Fd, FdInner},
};
use wasmer_wasix_types::wasi::Fd as WasiFd;

#[derive(Debug)]
pub struct FdList {
    fds: Vec<Option<Fd>>,
    first_free: Option<usize>,
}

/// Result of installing an fd. Replacements surface the old open-file
/// description only when that replacement dropped its final descriptor.
#[must_use = "replacement shutdown targets must be drained"]
pub(crate) struct InsertOutcome {
    pub inserted: bool,
    pub shutdown_target: Option<VirtualFileLock>,
}

/// An fd removed from the map together with the final-handle shutdown target,
/// if this removal closed the shared open-file description.
#[must_use = "final-handle shutdown targets must be drained"]
pub(crate) struct RemoveOutcome {
    pub fd: Fd,
    pub shutdown_target: Option<VirtualFileLock>,
}

pub struct FdListIterator<'a> {
    fds_iterator: core::slice::Iter<'a, Option<Fd>>,
    idx: usize,
}

pub struct FdListIteratorMut<'a> {
    fds_iterator: core::slice::IterMut<'a, Option<Fd>>,
    idx: usize,
}

impl Default for FdList {
    fn default() -> Self {
        Self::new()
    }
}

// TODO: rename all functions to something more sensible after all code is migrated
impl FdList {
    pub fn new() -> Self {
        Self {
            fds: vec![],
            first_free: None,
        }
    }

    pub fn next_free_fd(&self) -> WasiFd {
        match self.first_free {
            Some(i) => i as WasiFd,
            None => self.last_fd().map(|i| i + 1).unwrap_or(0),
        }
    }

    pub fn last_fd(&self) -> Option<WasiFd> {
        self.fds
            .iter()
            .rev()
            .position(|fd| fd.is_some())
            .map(|idx| (self.fds.len() - idx - 1) as WasiFd)
    }

    pub fn get(&self, idx: WasiFd) -> Option<&Fd> {
        self.fds.get(idx as usize).and_then(|x| x.as_ref())
    }

    pub fn get_mut(&mut self, idx: WasiFd) -> Option<&mut FdInner> {
        self.fds
            .get_mut(idx as usize)
            .and_then(|x| x.as_mut())
            .map(|x| &mut x.inner)
    }

    pub fn insert_first_free(&mut self, fd: Fd) -> WasiFd {
        fd.inode.acquire_handle();
        self.insert_first_free_preacquired(fd)
    }

    /// Installs an fd whose inode handle count was already incremented while
    /// holding that inode's write lock.
    pub(crate) fn insert_first_free_preacquired(&mut self, fd: Fd) -> WasiFd {
        match self.first_free {
            Some(free) => {
                assert!(self.fds[free].is_none());

                self.fds[free] = Some(fd);

                self.first_free = self.first_free_after(free as WasiFd + 1);

                free as WasiFd
            }
            None => {
                self.fds.push(Some(fd));
                (self.fds.len() - 1) as WasiFd
            }
        }
    }

    pub fn insert_first_free_after(&mut self, fd: Fd, after_or_equal: WasiFd) -> WasiFd {
        match self.first_free {
            // We're shorter than `after`, need to extend the list regardless of whether we have holes
            _ if self.fds.len() < after_or_equal as usize => {
                if !self.insert(true, after_or_equal, fd).inserted {
                    panic!(
                        "Internal error in FdList - expected {after_or_equal} to be unoccupied since the list wasn't long enough"
                    );
                }
                after_or_equal
            }

            // First free hole is suitable, we can insert there
            Some(free) if free >= after_or_equal as usize => self.insert_first_free(fd),

            // No holes, and we're longer than `after`, so insert at the end
            None if self.fds.len() >= after_or_equal as usize => self.insert_first_free(fd),

            // Keeping the compiler happy
            None => unreachable!("Both None cases were handled before"),

            // If there's a hole but its index is too low, we need to search
            Some(_) => {
                // This is handled by insert or insert_first_free in every other case, but not this one
                fd.inode.acquire_handle();

                match self.first_free_after(after_or_equal) {
                    // Found a suitable hole, and it's guaranteed to not be the first since
                    // that's checked in the previous Some case, so filling it has no effect
                    // on self.first_free
                    Some(free) => {
                        self.fds[free] = Some(fd);
                        free as WasiFd
                    }

                    // No holes - insert at the end
                    None => {
                        self.fds.push(Some(fd));
                        (self.fds.len() - 1) as WasiFd
                    }
                }
            }
        }
    }

    fn first_free_after(&self, after_or_equal: WasiFd) -> Option<usize> {
        let skip = after_or_equal as usize;
        self.fds
            .iter()
            .skip(skip)
            .position(|fd| fd.is_none())
            .map(|idx| idx + skip)
    }

    pub(crate) fn insert(&mut self, exclusive: bool, idx: WasiFd, fd: Fd) -> InsertOutcome {
        let idx = idx as usize;

        if self.fds.len() <= idx {
            if
            // if we have a first_free, it has to be before the end of the list, so
            // the only way for this to update first_free is if we don't have one at all
            self.first_free.is_none() &&
                // The target index must be at least len() + 1. If it's exactly len(),
                // it won't create a hole
                idx > self.fds.len()
            {
                self.first_free = Some(self.fds.len());
            }

            self.fds.resize(idx + 1, None);
        }

        if self.fds[idx].is_some() {
            if exclusive {
                return InsertOutcome {
                    inserted: false,
                    shutdown_target: None,
                };
            }
        }

        // Acquire the incoming reference before dropping a replaced one. If
        // both entries share an inode, this prevents a transient final-close
        // transition during dup2-style replacement.
        fd.inode.acquire_handle();
        let previous = self.fds[idx].replace(fd);
        let shutdown_target = previous.and_then(|fd| fd.inode.drop_one_handle());

        if self.first_free == Some(idx) {
            self.first_free = self.first_free_after(idx as WasiFd + 1);
        }

        InsertOutcome {
            inserted: true,
            shutdown_target,
        }
    }

    /// Installs a pre-acquired fd into an unoccupied exact slot.
    pub(crate) fn insert_preacquired(&mut self, idx: WasiFd, fd: Fd) -> bool {
        let idx = idx as usize;

        if self.fds.len() <= idx {
            if self.first_free.is_none() && idx > self.fds.len() {
                self.first_free = Some(self.fds.len());
            }
            self.fds.resize(idx + 1, None);
        }

        if self.fds[idx].is_some() {
            return false;
        }

        self.fds[idx] = Some(fd);
        if self.first_free == Some(idx) {
            self.first_free = self.first_free_after(idx as WasiFd + 1);
        }
        true
    }

    pub(crate) fn remove(&mut self, idx: WasiFd) -> Option<RemoveOutcome> {
        let idx = idx as usize;

        let fd = self.fds.get_mut(idx).and_then(|fd| fd.take())?;

        match self.first_free {
            None => self.first_free = Some(idx),
            Some(x) if x > idx => self.first_free = Some(idx),
            _ => (),
        }

        let shutdown_target = fd.inode.drop_one_handle();
        Some(RemoveOutcome {
            fd,
            shutdown_target,
        })
    }

    /// Removes every descriptor and returns all open-file descriptions whose
    /// final descriptor was dropped. The caller must drain them after releasing
    /// its fd-map lock.
    pub fn clear(&mut self) -> Vec<VirtualFileLock> {
        let mut shutdown_targets = Vec::new();
        for fd in self.fds.iter_mut().filter_map(Option::take) {
            if let Some(target) = fd.inode.drop_one_handle() {
                shutdown_targets.push(target);
            }
        }

        self.fds.clear();
        self.first_free = None;
        shutdown_targets
    }

    pub fn iter(&self) -> FdListIterator<'_> {
        FdListIterator {
            fds_iterator: self.fds.iter(),
            idx: 0,
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = WasiFd> + '_ {
        self.iter().map(|(key, _)| key)
    }

    pub fn iter_mut(&mut self) -> FdListIteratorMut<'_> {
        FdListIteratorMut {
            fds_iterator: self.fds.iter_mut(),
            idx: 0,
        }
    }
}

impl Clone for FdList {
    fn clone(&self) -> Self {
        for fd in &self.fds {
            if let Some(fd) = fd.as_ref() {
                fd.inode.acquire_handle();
            }
        }

        Self {
            fds: self.fds.clone(),
            first_free: self.first_free,
        }
    }
}

impl Drop for FdList {
    fn drop(&mut self) {
        // Async shutdown cannot be driven from Drop. Normal process/reinit
        // paths call `clear` explicitly and drain the returned targets first.
        let _ = self.clear();
    }
}

impl<'a> Iterator for FdListIterator<'a> {
    type Item = (WasiFd, &'a Fd);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.fds_iterator.next() {
                None => return None,

                Some(None) => {
                    self.idx += 1;
                    continue;
                }

                Some(Some(fd)) => {
                    let wasi_fd = self.idx as WasiFd;
                    self.idx += 1;
                    return Some((wasi_fd, fd));
                }
            }
        }
    }
}

impl<'a> Iterator for FdListIteratorMut<'a> {
    type Item = (WasiFd, &'a mut FdInner);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.fds_iterator.next() {
                None => return None,

                Some(None) => {
                    self.idx += 1;
                    continue;
                }

                Some(Some(fd)) => {
                    let wasi_fd = self.idx as WasiFd;
                    self.idx += 1;
                    return Some((wasi_fd, &mut fd.inner));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        borrow::Cow,
        io::{self, SeekFrom},
        pin::Pin,
        sync::{
            Arc, RwLock,
            atomic::{AtomicU64, AtomicUsize, Ordering},
            mpsc,
        },
        task::{Context, Poll},
        thread,
        time::{Duration, Instant},
    };

    use assert_panic::assert_panic;
    use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf};
    use virtual_fs::{FsError, Pipe, VirtualFile};
    use wasmer_wasix_types::wasi::{Fdflags, Fdflagsext, Rights};

    use crate::fs::{
        FlushPoller, HANDLE_CLOSED, HANDLE_FINALIZING, HandleAcquire, HandleDrop, Inode,
        InodeGuard, InodeVal, Kind, OpenHandleState, ShutdownPoller, fd::FdInner,
    };

    use super::{Fd, FdList, WasiFd};

    fn useless_fd(n: u16) -> Fd {
        Fd {
            open_flags: 0,
            inode: InodeGuard {
                ino: Inode(0),
                inner: Arc::new(InodeVal {
                    is_preopened: false,
                    kind: RwLock::new(Kind::Buffer { buffer: vec![] }),
                    name: RwLock::new(Cow::Borrowed("")),
                    stat: RwLock::new(Default::default()),
                }),
                open_handles: Arc::new(OpenHandleState::new()),
            },
            is_stdio: false,
            inner: FdInner {
                offset: Arc::new(AtomicU64::new(0)),
                rights: Rights::empty(),
                rights_inheriting: Rights::empty(),
                flags: Fdflags::from_bits_preserve(n),
                fd_flags: Fdflagsext::empty(),
            },
        }
    }

    #[derive(Debug)]
    struct CountingFile {
        flushes: Arc<AtomicUsize>,
        shutdowns: Arc<AtomicUsize>,
    }

    impl AsyncRead for CountingFile {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncWrite for CountingFile {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.flushes.fetch_add(1, Ordering::SeqCst);
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.shutdowns.fetch_add(1, Ordering::SeqCst);
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncSeek for CountingFile {
        fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> io::Result<()> {
            Ok(())
        }

        fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
            Poll::Ready(Ok(0))
        }
    }

    impl VirtualFile for CountingFile {
        fn last_accessed(&self) -> u64 {
            0
        }

        fn last_modified(&self) -> u64 {
            0
        }

        fn created_time(&self) -> u64 {
            0
        }

        fn size(&self) -> u64 {
            0
        }

        fn set_len(&mut self, _new_size: u64) -> Result<(), FsError> {
            Ok(())
        }

        fn unlink(&mut self) -> Result<(), FsError> {
            Ok(())
        }

        fn poll_read_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(0))
        }

        fn poll_write_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(8192))
        }
    }

    fn counting_fd(n: u16) -> (Fd, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let flushes = Arc::new(AtomicUsize::new(0));
        let shutdowns = Arc::new(AtomicUsize::new(0));
        let file = CountingFile {
            flushes: flushes.clone(),
            shutdowns: shutdowns.clone(),
        };
        let fd = Fd {
            open_flags: 0,
            inode: InodeGuard {
                ino: Inode(n as u64),
                inner: Arc::new(InodeVal {
                    is_preopened: false,
                    kind: RwLock::new(Kind::File {
                        handle: Some(Arc::new(RwLock::new(Box::new(file)))),
                        path: "".into(),
                        fd: None,
                    }),
                    name: RwLock::new(Cow::Borrowed("counting")),
                    stat: RwLock::new(Default::default()),
                }),
                open_handles: Arc::new(OpenHandleState::new()),
            },
            is_stdio: false,
            inner: FdInner {
                offset: Arc::new(AtomicU64::new(0)),
                rights: Rights::FD_SYNC | Rights::FD_DATASYNC,
                rights_inheriting: Rights::empty(),
                flags: Fdflags::empty(),
                fd_flags: Fdflagsext::empty(),
            },
        };
        (fd, flushes, shutdowns)
    }

    fn drain_shutdown(target: super::VirtualFileLock) {
        virtual_mio::block_on(ShutdownPoller { file: target }).unwrap();
    }

    fn is_useless_fd(fd: &Fd, n: u16) -> bool {
        fd.inner.flags.bits() == n
    }

    fn is_useless_fd_inner(fd_inner: &FdInner, n: u16) -> bool {
        fd_inner.flags.bits() == n
    }

    fn assert_fds_match(l: &FdList, expected: &[(WasiFd, u16)]) {
        let mut i = l.iter();

        for e in expected {
            let next = i.next().expect("Should have a next element");
            assert_eq!(next.0, e.0);
            assert!(is_useless_fd(next.1, e.1));
        }

        assert!(i.next().is_none());
    }

    #[test]
    fn can_append_fds() {
        let mut l = FdList::new();
        l.insert_first_free(useless_fd(0));
        l.insert_first_free(useless_fd(1));

        assert_fds_match(&l, &[(0, 0), (1, 1)]);
    }

    #[test]
    fn can_append_in_holes() {
        let mut l = FdList::new();
        l.insert_first_free(useless_fd(0));
        l.insert_first_free(useless_fd(1));
        l.insert_first_free(useless_fd(2));
        l.insert_first_free(useless_fd(3));
        let _ = l.remove(1);
        let _ = l.remove(2);
        l.insert_first_free(useless_fd(4));

        assert_fds_match(&l, &[(0, 0), (1, 4), (3, 3)]);
    }

    #[test]
    fn can_have_holes_in_different_places() {
        let mut l = FdList::new();
        l.insert_first_free(useless_fd(0));
        l.insert_first_free(useless_fd(1));
        l.insert_first_free(useless_fd(2));
        l.insert_first_free(useless_fd(3));
        l.insert_first_free(useless_fd(4));
        let _ = l.remove(1);
        let _ = l.remove(3);
        l.insert_first_free(useless_fd(5));
        l.insert_first_free(useless_fd(6));

        assert_fds_match(&l, &[(0, 0), (1, 5), (2, 2), (3, 6), (4, 4)]);
    }

    #[test]
    fn hole_moves_back_correctly() {
        let mut l = FdList::new();
        l.insert_first_free(useless_fd(0));
        l.insert_first_free(useless_fd(1));
        l.insert_first_free(useless_fd(2));
        l.insert_first_free(useless_fd(3));
        let _ = l.remove(3);
        assert_eq!(l.first_free, Some(3));
        let _ = l.remove(1);
        assert_eq!(l.first_free, Some(1));
        l.insert_first_free(useless_fd(4));

        assert_fds_match(&l, &[(0, 0), (1, 4), (2, 2)]);
    }

    #[test]
    fn insert_at_first_free_updates_first_free() {
        let mut l = FdList::new();
        l.insert_first_free(useless_fd(0));
        l.insert_first_free(useless_fd(1));
        l.insert_first_free(useless_fd(2));
        l.insert_first_free(useless_fd(3));
        let _ = l.remove(1);
        let _ = l.remove(2);
        assert!(l.insert(true, 1, useless_fd(4)).inserted);
        assert_eq!(l.first_free, Some(2));

        assert_fds_match(&l, &[(0, 0), (1, 4), (3, 3)]);
    }

    #[test]
    fn next_and_last_fd_reported_correctly() {
        let mut l = FdList::new();

        assert_eq!(l.next_free_fd(), 0);
        assert_eq!(l.last_fd(), None);

        l.insert_first_free(useless_fd(0));
        l.insert_first_free(useless_fd(1));

        assert_eq!(l.next_free_fd(), 2);
        assert_eq!(l.last_fd(), Some(1));

        l.insert_first_free(useless_fd(2));
        l.insert_first_free(useless_fd(3));

        assert_eq!(l.next_free_fd(), 4);
        assert_eq!(l.last_fd(), Some(3));

        let _ = l.remove(3);

        assert_eq!(l.next_free_fd(), 3);
        assert_eq!(l.last_fd(), Some(2));

        let _ = l.remove(1);

        assert_eq!(l.next_free_fd(), 1);
        assert_eq!(l.last_fd(), Some(2));
    }

    #[test]
    fn get_works() {
        let mut l = FdList::new();

        l.insert_first_free(useless_fd(0));
        l.insert_first_free(useless_fd(1));
        l.insert_first_free(useless_fd(2));
        l.insert_first_free(useless_fd(3));
        l.insert_first_free(useless_fd(4));
        let _ = l.remove(1);
        let _ = l.remove(3);

        assert!(l.get(1).is_none());
        assert!(is_useless_fd(l.get(2).unwrap(), 2));

        let at_4 = l.get_mut(4).unwrap();
        assert!(is_useless_fd_inner(at_4, 4));
        at_4.flags = Fdflags::from_bits_preserve(5); // Update the "useless FD" number without changing the InodeGuard
        assert!(is_useless_fd(l.get(4).unwrap(), 5));

        assert!(l.get(10).is_none());
        assert!(l.get_mut(10).is_none());
    }

    #[test]
    fn insert_at_works() {
        let mut l = FdList::new();

        l.insert_first_free(useless_fd(0));
        l.insert_first_free(useless_fd(1));
        l.insert_first_free(useless_fd(2));
        let _ = l.remove(1);

        assert!(l.insert(false, 2, useless_fd(3)).inserted);
        assert!(is_useless_fd(l.get(2).unwrap(), 3));

        assert!(!l.insert(true, 2, useless_fd(4)).inserted);
        assert!(is_useless_fd(l.get(2).unwrap(), 3));

        assert!(l.insert(true, 1, useless_fd(5)).inserted);
        assert!(is_useless_fd(l.get(1).unwrap(), 5));
    }

    #[test]
    fn insert_at_can_insert_beyond_end_of_list() {
        let mut l = FdList::new();

        l.insert_first_free(useless_fd(0));

        assert!(l.insert(false, 1, useless_fd(1)).inserted);
        assert!(is_useless_fd(l.get(1).unwrap(), 1));

        // Extending by exactly one element shouldn't change first_free
        assert_eq!(l.last_fd(), Some(1));
        assert_eq!(l.next_free_fd(), 2);
        assert!(l.first_free.is_none());

        // Now create a hole
        assert!(l.insert(false, 5, useless_fd(5)).inserted);
        assert!(is_useless_fd(l.get(5).unwrap(), 5));

        for i in 2..=4 {
            assert!(l.get(i).is_none());
        }

        // Creating a hole should update first_free
        assert_eq!(l.last_fd(), Some(5));
        assert_eq!(l.next_free_fd(), 2);
        assert_eq!(l.first_free, Some(2));
    }

    #[test]
    fn insert_first_free_after_beyond_end_of_empty_list() {
        let mut l = FdList::new();
        assert_eq!(l.insert_first_free_after(useless_fd(1), 5), 5);
        assert!(is_useless_fd(l.get(5).unwrap(), 1));
    }

    #[test]
    fn insert_first_free_after_beyond_end_of_non_empty_list() {
        let mut l = FdList::new();
        assert!(l.insert(false, 0, useless_fd(0)).inserted);
        assert_eq!(l.insert_first_free_after(useless_fd(1), 5), 5);
        assert!(is_useless_fd(l.get(5).unwrap(), 1));
    }

    #[test]
    fn insert_first_free_after_beyond_end_of_non_empty_list_with_hole() {
        let mut l = FdList::new();
        assert!(l.insert(false, 0, useless_fd(0)).inserted);
        assert!(l.insert(false, 2, useless_fd(2)).inserted);
        assert_eq!(l.insert_first_free_after(useless_fd(1), 5), 5);
        assert!(is_useless_fd(l.get(5).unwrap(), 1));
    }

    #[test]
    fn insert_first_free_after_behind_hole() {
        let mut l = FdList::new();
        l.insert_first_free(useless_fd(0));
        l.insert_first_free(useless_fd(1));
        l.insert_first_free(useless_fd(2));
        l.insert_first_free(useless_fd(3));
        let _ = l.remove(2).unwrap();
        assert_eq!(l.insert_first_free_after(useless_fd(5), 1), 2);
        assert!(is_useless_fd(l.get(2).unwrap(), 5));
    }

    #[test]
    fn insert_first_free_after_behind_end_without_hole() {
        let mut l = FdList::new();
        l.insert_first_free(useless_fd(0));
        l.insert_first_free(useless_fd(1));
        l.insert_first_free(useless_fd(2));
        l.insert_first_free(useless_fd(3));
        assert_eq!(l.insert_first_free_after(useless_fd(5), 2), 4);
        assert!(is_useless_fd(l.get(4).unwrap(), 5));
    }

    #[test]
    fn insert_first_free_after_between_hole_and_end_without_other_hole() {
        let mut l = FdList::new();
        l.insert_first_free(useless_fd(0));
        l.insert_first_free(useless_fd(1));
        l.insert_first_free(useless_fd(2));
        l.insert_first_free(useless_fd(3));
        l.insert_first_free(useless_fd(4));
        let _ = l.remove(1).unwrap();
        assert_eq!(l.insert_first_free_after(useless_fd(5), 2), 5);
        assert!(is_useless_fd(l.get(5).unwrap(), 5));
    }

    #[test]
    fn insert_first_free_after_between_hole_and_end_with_other_hole() {
        let mut l = FdList::new();
        l.insert_first_free(useless_fd(0));
        l.insert_first_free(useless_fd(1));
        l.insert_first_free(useless_fd(2));
        l.insert_first_free(useless_fd(3));
        l.insert_first_free(useless_fd(4));
        let _ = l.remove(1).unwrap();
        let _ = l.remove(3).unwrap();
        assert_eq!(l.insert_first_free_after(useless_fd(5), 2), 3);
        assert!(is_useless_fd(l.get(3).unwrap(), 5));
    }

    #[test]
    fn remove_works() {
        let mut l = FdList::new();

        l.insert_first_free(useless_fd(0));
        l.insert_first_free(useless_fd(1));
        l.insert_first_free(useless_fd(2));

        assert!(is_useless_fd(&l.remove(1).unwrap().fd, 1));
        assert!(l.remove(1).is_none());
        assert!(l.remove(100000).is_none());
    }

    #[test]
    fn clear_works() {
        let mut l = FdList::new();

        l.insert_first_free(useless_fd(0));
        l.insert_first_free(useless_fd(1));
        l.insert_first_free(useless_fd(2));
        let _ = l.remove(1);

        assert!(l.clear().is_empty());

        assert_eq!(l.next_free_fd(), 0);
        assert!(l.last_fd().is_none());
        assert_eq!(l.fds.len(), 0);
        assert!(l.first_free.is_none());
    }

    #[test]
    fn iter_mut_works() {
        let mut l = FdList::new();
        l.insert_first_free(useless_fd(0));
        l.insert_first_free(useless_fd(1));

        let mut i = l.iter_mut();

        let next = i.next().unwrap();
        assert_eq!(next.0, 0);
        assert!(is_useless_fd_inner(next.1, 0));
        next.1.flags = Fdflags::from_bits_preserve(2); // Update the "useless FD" number without changing the InodeGuard

        let next = i.next().unwrap();
        assert_eq!(next.0, 1);
        assert!(is_useless_fd_inner(next.1, 1));

        assert!(i.next().is_none());

        assert_fds_match(&l, &[(0, 2), (1, 1)]);
    }

    #[test]
    fn open_handles_are_updated_correctly() {
        let mut l = FdList::new();
        l.insert_first_free(useless_fd(0));
        l.insert_first_free(useless_fd(1));

        let fd0 = l.get(0).unwrap().clone();
        assert_eq!(fd0.inode.handle_count(), 1);

        // Try removing an FD, should drop the handle
        let fd1 = l.get(1).unwrap().clone();
        assert_eq!(fd1.inode.handle_count(), 1);
        let _ = l.remove(1).unwrap();
        assert_eq!(fd1.inode.handle_count(), 0);

        // Existing FDs should get a new handle when cloning the list
        let mut l2 = l.clone();
        assert_eq!(fd0.inode.handle_count(), 2);

        {
            // Dropping the list should drop open handles
            let l3 = l2.clone();
            assert_eq!(fd0.inode.handle_count(), 3);
            drop(l3);
            assert_eq!(fd0.inode.handle_count(), 2);
        }

        // Clearing the list should drop open handles
        assert!(l.clear().is_empty());
        assert_eq!(fd0.inode.handle_count(), 1);

        // Clear the last handle, should go back to zero
        assert!(l2.clear().is_empty());
        assert_eq!(fd0.inode.handle_count(), 0);

        assert_panic!(
            {
                let _ = fd0.inode.drop_one_handle();
            },
            &str,
            "InodeGuard handle dropped too many times"
        );

        assert_panic!(drop(fd0.inode.write()), String, contains "PoisonError");
    }

    #[test]
    fn duplicated_descriptor_shutdowns_only_on_final_close() {
        let (fd, flushes, shutdowns) = counting_fd(1);
        let duplicate = fd.clone();
        let mut fds = FdList::new();
        fds.insert_first_free(fd);
        fds.insert_first_free(duplicate);

        let first = fds.remove(0).unwrap();
        assert!(first.shutdown_target.is_none());
        assert_eq!(first.fd.inode.handle_count(), 1);
        assert_eq!(shutdowns.load(Ordering::SeqCst), 0);

        let last = fds.remove(1).unwrap();
        assert_eq!(last.fd.inode.handle_count(), 0);
        drain_shutdown(last.shutdown_target.unwrap());
        assert_eq!(shutdowns.load(Ordering::SeqCst), 1);
        assert_eq!(flushes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn forked_fd_map_shutdowns_only_after_both_processes_close() {
        let (fd, _flushes, shutdowns) = counting_fd(2);
        let mut parent = FdList::new();
        parent.insert_first_free(fd);
        let mut child = parent.clone();

        let parent_close = parent.remove(0).unwrap();
        assert!(parent_close.shutdown_target.is_none());
        assert_eq!(parent_close.fd.inode.handle_count(), 1);

        let child_close = child.remove(0).unwrap();
        assert_eq!(child_close.fd.inode.handle_count(), 0);
        drain_shutdown(child_close.shutdown_target.unwrap());
        assert_eq!(shutdowns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn replacement_returns_only_replaced_final_description() {
        let (source, _source_flushes, source_shutdowns) = counting_fd(3);
        let (destination, _destination_flushes, destination_shutdowns) = counting_fd(4);
        let mut fds = FdList::new();
        fds.insert_first_free(source);
        fds.insert_first_free(destination);

        let duplicate = fds.get(0).unwrap().clone();
        let replacement = fds.insert(false, 1, duplicate);
        assert!(replacement.inserted);
        drain_shutdown(replacement.shutdown_target.unwrap());
        assert_eq!(destination_shutdowns.load(Ordering::SeqCst), 1);
        assert_eq!(source_shutdowns.load(Ordering::SeqCst), 0);

        assert!(fds.remove(0).unwrap().shutdown_target.is_none());
        let final_source = fds.remove(1).unwrap().shutdown_target.unwrap();
        drain_shutdown(final_source);
        assert_eq!(source_shutdowns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn explicit_sync_flush_is_distinct_from_close_shutdown() {
        let (fd, flushes, shutdowns) = counting_fd(5);
        let file = {
            let guard = fd.inode.read();
            match &*guard {
                Kind::File {
                    handle: Some(file), ..
                } => file.clone(),
                _ => unreachable!(),
            }
        };
        let mut fds = FdList::new();
        fds.insert_first_free(fd);

        virtual_mio::block_on(FlushPoller { file }).unwrap();
        assert_eq!(flushes.load(Ordering::SeqCst), 1);
        assert_eq!(shutdowns.load(Ordering::SeqCst), 0);

        drain_shutdown(fds.remove(0).unwrap().shutdown_target.unwrap());
        assert_eq!(flushes.load(Ordering::SeqCst), 1);
        assert_eq!(shutdowns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn nonfinal_handle_changes_do_not_wait_for_inode_lock() {
        let (fd, _flushes, _shutdowns) = counting_fd(6);
        let inode = fd.inode.clone();
        inode.acquire_handle();
        assert_eq!(inode.handle_count(), 1);

        // Holding the heavyweight inode lock must not stall OPEN(n) acquire or
        // an OPEN(n>1) drop. This is the descriptor hot-path regression guard.
        let inode_guard = inode.write();
        let worker_inode = inode.clone();
        let (tx, rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            worker_inode.acquire_handle();
            tx.send(worker_inode.handle_count()).unwrap();
            assert!(worker_inode.drop_one_handle().is_none());
            tx.send(worker_inode.handle_count()).unwrap();
        });

        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), 2);
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
        drop(inode_guard);
        worker.join().unwrap();

        drain_shutdown(inode.drop_one_handle().unwrap());
    }

    #[test]
    fn finalizing_state_is_rescuable_but_owned_state_is_not() {
        let state = OpenHandleState::new();
        assert_eq!(state.try_acquire(), HandleAcquire::Acquired(1));
        assert_eq!(state.begin_drop(), HandleDrop::Finalizing);
        assert_eq!(state.raw(), HANDLE_FINALIZING);
        assert_eq!(state.count(), 0);

        // An acquire linearized before final ownership rescues the resource.
        assert_eq!(state.try_acquire(), HandleAcquire::Acquired(1));
        assert!(!state.try_own_final());
        assert_eq!(state.count(), 1);

        // Once ownership wins, ordinary acquisition must not silently create
        // a descriptor; locked activation is required after final completion.
        assert_eq!(state.begin_drop(), HandleDrop::Finalizing);
        assert!(state.try_own_final());
        assert_eq!(state.count(), 0);
        assert_eq!(state.try_acquire(), HandleAcquire::NeedsLockedActivation);
        state.finish_final(true);
        assert_eq!(state.raw(), HANDLE_CLOSED);
        assert_eq!(state.count(), 0);
        assert_eq!(state.try_acquire(), HandleAcquire::NeedsLockedActivation);
    }

    #[test]
    fn handle_count_overflow_panics_instead_of_wrapping() {
        let state = OpenHandleState::new();
        state.state.store(i32::MAX, Ordering::Release);
        assert_panic!(
            {
                let _ = state.try_acquire();
            },
            &str,
            "InodeGuard handle count overflow"
        );
        assert_eq!(state.raw(), i32::MAX);
    }

    #[test]
    fn locked_path_open_can_rescue_a_waiting_final_drop() {
        let (fd, _flushes, shutdowns) = counting_fd(7);
        let inode = fd.inode.clone();
        inode.acquire_handle();

        // The closer can nominate the final transition while path_open holds
        // the inode lock, but cannot own or extract the handle yet.
        let inode_guard = inode.write();
        let closer_inode = inode.clone();
        let closer = thread::spawn(move || closer_inode.drop_one_handle());

        let deadline = Instant::now() + Duration::from_secs(5);
        while inode.open_handles.raw() != HANDLE_FINALIZING {
            assert!(Instant::now() < deadline, "closer never reached FINALIZING");
            thread::yield_now();
        }

        inode.acquire_handle_locked(&inode_guard);
        assert_eq!(inode.handle_count(), 1);
        drop(inode_guard);
        assert!(closer.join().unwrap().is_none());

        drain_shutdown(inode.drop_one_handle().unwrap());
        assert_eq!(shutdowns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn completed_final_close_requires_handle_install_before_locked_reopen() {
        let (fd, _flushes, old_shutdowns) = counting_fd(8);
        let inode = fd.inode.clone();
        inode.acquire_handle();
        drain_shutdown(inode.drop_one_handle().unwrap());
        assert_eq!(old_shutdowns.load(Ordering::SeqCst), 1);
        assert_eq!(inode.open_handles.raw(), HANDLE_CLOSED);

        let new_flushes = Arc::new(AtomicUsize::new(0));
        let new_shutdowns = Arc::new(AtomicUsize::new(0));
        let replacement = CountingFile {
            flushes: new_flushes,
            shutdowns: new_shutdowns.clone(),
        };
        {
            let mut guard = inode.write();
            let Kind::File { handle, .. } = &mut *guard else {
                unreachable!();
            };
            assert!(handle.is_none());
            *handle = Some(Arc::new(RwLock::new(Box::new(replacement))));
            inode.acquire_handle_locked(&guard);
        }
        assert_eq!(inode.handle_count(), 1);

        drain_shutdown(inode.drop_one_handle().unwrap());
        assert_eq!(new_shutdowns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn closed_file_cannot_reactivate_before_replacement_handle_is_installed() {
        let (fd, _flushes, _shutdowns) = counting_fd(9);
        let inode = fd.inode.clone();
        inode.acquire_handle();
        let _target = inode.drop_one_handle().unwrap();
        assert_eq!(inode.open_handles.raw(), HANDLE_CLOSED);

        assert_panic!(
            {
                let guard = inode.write();
                inode.acquire_handle_locked(&guard);
            },
            &str,
            "cannot reactivate a closed file before installing its handle"
        );
        assert_eq!(inode.open_handles.raw(), HANDLE_CLOSED);
    }

    #[test]
    fn ordinary_acquire_cannot_attach_to_a_closed_or_later_reopened_file() {
        let (fd, _flushes, _shutdowns) = counting_fd(10);
        let inode = fd.inode.clone();
        inode.acquire_handle();
        let _target = inode.drop_one_handle().unwrap();
        assert_eq!(inode.open_handles.raw(), HANDLE_CLOSED);

        assert_panic!(
            inode.acquire_handle(),
            &str,
            "ordinary handle acquisition requires locked reactivation"
        );
        assert_eq!(inode.open_handles.raw(), HANDLE_CLOSED);
    }

    #[test]
    fn terminal_pipe_cannot_be_resurrected() {
        let (tx, _rx) = Pipe::new().split();
        let mut fd = useless_fd(11);
        fd.inode.inner = Arc::new(InodeVal {
            is_preopened: false,
            kind: RwLock::new(Kind::PipeTx { tx }),
            name: RwLock::new(Cow::Borrowed("pipe")),
            stat: RwLock::new(Default::default()),
        });
        let inode = fd.inode.clone();
        inode.acquire_handle();
        assert!(inode.drop_one_handle().is_none());
        assert_eq!(inode.open_handles.raw(), HANDLE_CLOSED);

        assert_panic!(
            {
                let guard = inode.write();
                inode.acquire_handle_locked(&guard);
            },
            &str,
            "cannot reactivate a terminal inode resource"
        );
        assert_eq!(inode.open_handles.raw(), HANDLE_CLOSED);
    }

    #[test]
    fn resource_retaining_kind_returns_to_ready() {
        let fd = useless_fd(12);
        let inode = fd.inode.clone();
        inode.acquire_handle();
        assert!(inode.drop_one_handle().is_none());
        assert_eq!(inode.open_handles.raw(), 0);

        inode.acquire_handle();
        assert_eq!(inode.handle_count(), 1);
        assert!(inode.drop_one_handle().is_none());
        assert_eq!(inode.open_handles.raw(), 0);
    }

    #[test]
    fn messing_with_inode_causes_panic() {
        // We want to pin this behavior down, as not causing a panic
        // can lead to inconsistencies
        let mut l = FdList::new();
        l.insert_first_free(useless_fd(0));

        let fd = l.get(0).unwrap();
        let _ = fd.inode.drop_one_handle();

        assert_panic!(drop(l), &str, "InodeGuard handle dropped too many times");
    }
}
