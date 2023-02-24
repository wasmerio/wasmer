use std::convert::TryFrom;
use std::fmt;
use std::mem;

pub struct Table<T> {
    elems: Vec<Slot<T>>,
    next: usize,
}

#[derive(Debug)]
pub enum RemoveError {
    NotAllocated,
}

enum Slot<T> {
    Empty { next_empty: usize },
    Full { item: Box<T> },
}

impl<T> Table<T> {
    /// Creates a new empty table
    pub fn new() -> Table<T> {
        Table {
            elems: Vec::new(),
            next: 0,
        }
    }

    /// Inserts an item into this table, returning the index that it was
    /// inserted at.
    pub fn insert(&mut self, item: T) -> u32 {
        if self.next == self.elems.len() {
            let next_empty = self.next + 1;
            self.elems.push(Slot::Empty { next_empty });
        }
        let index = self.next;
        let ret = u32::try_from(index).unwrap();
        self.next = match &self.elems[index] {
            Slot::Empty { next_empty } => *next_empty,
            Slot::Full { .. } => unreachable!(),
        };
        self.elems[index] = Slot::Full {
            item: Box::new(item),
        };
        ret
    }

    /// Borrows an item from this table.
    ///
    /// Returns `None` if the index is not allocated at this time. Otherwise
    /// returns `Some` with a borrow of the item from this table.
    pub fn get(&self, item: u32) -> Option<&T> {
        let index = usize::try_from(item).unwrap();
        match self.elems.get(index)? {
            Slot::Empty { .. } => None,
            Slot::Full { item } => Some(item),
        }
    }

    /// Removes an item from this table.
    ///
    /// On success it returns back the original item.
    pub fn remove(&mut self, item: u32) -> Result<T, RemoveError> {
        let index = usize::try_from(item).unwrap();
        let new_empty = Slot::Empty {
            next_empty: self.next,
        };
        let slot = self.elems.get_mut(index).ok_or(RemoveError::NotAllocated)?;

        // Assume that `item` is valid, and if it is, we can return quickly
        match mem::replace(slot, new_empty) {
            Slot::Full { item } => {
                self.next = index;
                Ok(*item)
            }

            // Oops `item` wasn't valid, put it back where we found it and then
            // figure out why it was invalid
            Slot::Empty { next_empty } => {
                *slot = Slot::Empty { next_empty };
                Err(RemoveError::NotAllocated)
            }
        }
    }
}

impl<T> Default for Table<T> {
    fn default() -> Table<T> {
        Table::new()
    }
}

impl<T> fmt::Debug for Table<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Table")
            .field("capacity", &self.elems.capacity())
            .finish()
    }
}

impl fmt::Display for RemoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemoveError::NotAllocated => f.write_str("invalid handle index"),
        }
    }
}

impl std::error::Error for RemoveError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple() {
        let mut table = Table::new();
        assert_eq!(table.insert(0), 0);
        assert_eq!(table.insert(100), 1);
        assert_eq!(table.insert(200), 2);

        assert_eq!(*table.get(0).unwrap(), 0);
        assert_eq!(*table.get(1).unwrap(), 100);
        assert_eq!(*table.get(2).unwrap(), 200);
        assert!(table.get(100).is_none());

        assert!(table.remove(0).is_ok());
        assert!(table.get(0).is_none());
        assert_eq!(table.insert(1), 0);
        assert!(table.get(0).is_some());

        table.get(1).unwrap();
        assert!(table.remove(1).is_ok());
        assert!(table.remove(1).is_err());

        assert!(table.remove(2).is_ok());
        assert!(table.remove(0).is_ok());

        assert_eq!(table.insert(100), 0);
        assert_eq!(table.insert(100), 2);
        assert_eq!(table.insert(100), 1);
        assert_eq!(table.insert(100), 3);
    }
}
