//! A very simple data structure for holding the FDs a WASI process is using.
//! Keeps track of the first unused (i.e. freed) FD, which is slightly faster
//! than doing a linear search of the entire array each time.
//! Note, The Unix spec requires newly allocated FDs to always be the
//! lowest-numbered FD available.

use super::fd::Fd;
use wasmer_wasix_types::wasi::Fd as WasiFd;

#[derive(Debug, Clone)]
pub struct FdList {
    fds: Vec<Option<Fd>>,
    first_free: Option<usize>,
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

    pub fn get_mut(&mut self, idx: WasiFd) -> Option<&mut Fd> {
        self.fds.get_mut(idx as usize).and_then(|x| x.as_mut())
    }

    pub fn insert_first_free(&mut self, fd: Fd) -> WasiFd {
        match self.first_free {
            Some(free) => {
                debug_assert!(self.fds[free].is_none());

                self.fds[free] = Some(fd);

                self.first_free = self
                    .fds
                    .iter()
                    .skip(free + 1)
                    .position(|fd| fd.is_none())
                    .map(|idx| idx + free + 1);

                free as WasiFd
            }
            None => {
                self.fds.push(Some(fd));
                (self.fds.len() - 1) as WasiFd
            }
        }
    }

    pub fn insert(&mut self, exclusive: bool, idx: WasiFd, fd: Fd) -> bool {
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

        if self.fds[idx].is_some() && exclusive {
            return false;
        }

        self.fds[idx] = Some(fd);
        true
    }

    pub fn remove(&mut self, idx: WasiFd) -> Option<Fd> {
        let idx = idx as usize;

        let result = self.fds[idx].take();

        if result.is_some() {
            match self.first_free {
                None => self.first_free = Some(idx),
                Some(x) if x > idx => self.first_free = Some(idx),
                _ => (),
            }
        }

        result
    }

    pub fn clear(&mut self) {
        self.fds.clear();
        self.first_free = None;
    }

    pub fn iter(&self) -> FdListIterator {
        FdListIterator {
            fds_iterator: self.fds.iter(),
            idx: 0,
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = WasiFd> + '_ {
        self.iter().map(|(key, _)| key)
    }

    pub fn iter_mut(&mut self) -> FdListIteratorMut {
        FdListIteratorMut {
            fds_iterator: self.fds.iter_mut(),
            idx: 0,
        }
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
    type Item = (WasiFd, &'a mut Fd);

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

#[cfg(test)]
mod tests {
    use std::{
        borrow::Cow,
        sync::{atomic::AtomicU64, Arc, RwLock},
    };

    use wasmer_wasix_types::wasi::{Fdflags, Rights};

    use crate::fs::{Inode, InodeGuard, InodeVal, Kind};

    use super::{Fd, FdList, WasiFd};

    fn useless_fd(n: u16) -> Fd {
        Fd {
            open_flags: n,
            flags: Fdflags::empty(),
            inode: InodeGuard {
                ino: Inode(0),
                inner: Arc::new(InodeVal {
                    is_preopened: false,
                    kind: RwLock::new(Kind::Buffer { buffer: vec![] }),
                    name: Cow::Borrowed(""),
                    stat: RwLock::new(Default::default()),
                }),
            },
            is_stdio: false,
            offset: Arc::new(AtomicU64::new(0)),
            rights: Rights::empty(),
            rights_inheriting: Rights::empty(),
        }
    }

    fn is_useless_fd(fd: &Fd, n: u16) -> bool {
        fd.open_flags == n
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
        l.remove(1);
        l.remove(2);
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
        l.remove(1);
        l.remove(3);
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
        l.remove(3);
        assert_eq!(l.first_free, Some(3));
        l.remove(1);
        assert_eq!(l.first_free, Some(1));
        l.insert_first_free(useless_fd(4));

        assert_fds_match(&l, &[(0, 0), (1, 4), (2, 2)]);
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

        l.remove(3);

        assert_eq!(l.next_free_fd(), 3);
        assert_eq!(l.last_fd(), Some(2));

        l.remove(1);

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
        l.remove(1);
        l.remove(3);

        assert!(l.get(1).is_none());
        assert!(is_useless_fd(l.get(2).unwrap(), 2));

        let at_4 = l.get_mut(4).unwrap();
        assert!(is_useless_fd(at_4, 4));
        *at_4 = useless_fd(5);
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
        l.remove(1);

        assert!(l.insert(false, 2, useless_fd(3)));
        assert!(is_useless_fd(l.get(2).unwrap(), 3));

        assert!(!l.insert(true, 2, useless_fd(4)));
        assert!(is_useless_fd(l.get(2).unwrap(), 3));

        assert!(l.insert(true, 1, useless_fd(5)));
        assert!(is_useless_fd(l.get(1).unwrap(), 5));
    }

    #[test]
    fn insert_at_can_insert_beyond_end_of_list() {
        let mut l = FdList::new();

        l.insert_first_free(useless_fd(0));

        assert!(l.insert(false, 1, useless_fd(1)));
        assert!(is_useless_fd(l.get(1).unwrap(), 1));

        // Extending by exactly one element shouldn't change first_free
        assert_eq!(l.last_fd(), Some(1));
        assert_eq!(l.next_free_fd(), 2);
        assert!(l.first_free.is_none());

        // Now create a hole
        assert!(l.insert(false, 5, useless_fd(5)));
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
    fn clear_works() {
        let mut l = FdList::new();

        l.insert_first_free(useless_fd(0));
        l.insert_first_free(useless_fd(1));
        l.insert_first_free(useless_fd(2));
        l.remove(1);

        l.clear();

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
        assert!(is_useless_fd(next.1, 0));
        *next.1 = useless_fd(2);

        let next = i.next().unwrap();
        assert_eq!(next.0, 1);
        assert!(is_useless_fd(next.1, 1));

        assert!(i.next().is_none());

        assert_fds_match(&l, &[(0, 2), (1, 1)]);
    }
}
