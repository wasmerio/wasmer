use std::fmt;
use std::mem;

pub struct Slab<T> {
    storage: Vec<Entry<T>>,
    next: usize,
}

enum Entry<T> {
    Full(T),
    Empty { next: usize },
}

impl<T> Slab<T> {
    pub fn insert(&mut self, item: T) -> u32 {
        if self.next == self.storage.len() {
            self.storage.push(Entry::Empty {
                next: self.next + 1,
            });
        }
        let ret = self.next as u32;
        let entry = Entry::Full(item);
        self.next = match mem::replace(&mut self.storage[self.next], entry) {
            Entry::Empty { next } => next,
            _ => unreachable!(),
        };
        ret
    }

    pub fn get(&self, idx: u32) -> Option<&T> {
        match self.storage.get(idx as usize)? {
            Entry::Full(b) => Some(b),
            Entry::Empty { .. } => None,
        }
    }

    pub fn get_mut(&mut self, idx: u32) -> Option<&mut T> {
        match self.storage.get_mut(idx as usize)? {
            Entry::Full(b) => Some(b),
            Entry::Empty { .. } => None,
        }
    }

    pub fn remove(&mut self, idx: u32) -> Option<T> {
        let slot = self.storage.get_mut(idx as usize)?;
        match mem::replace(slot, Entry::Empty { next: self.next }) {
            Entry::Full(b) => {
                self.next = idx as usize;
                Some(b)
            }
            Entry::Empty { next } => {
                *slot = Entry::Empty { next };
                None
            }
        }
    }
}

impl<T> Default for Slab<T> {
    fn default() -> Slab<T> {
        Slab {
            storage: Vec::new(),
            next: 0,
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Slab<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Slab").finish()
    }
}
