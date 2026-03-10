use super::{Iter, IterMut, TypedIndex};
use std::{
    marker::PhantomData,
    ops::{Index, IndexMut},
};

/// This is a dynamically-sized slice
/// that can only be indexed by the
/// correct index type.
#[derive(Debug)]
pub struct SliceMap<K, V>
where
    K: TypedIndex,
{
    _marker: PhantomData<K>,
    slice: [V],
}

impl<K, V> SliceMap<K, V>
where
    K: TypedIndex,
{
    /// Gets a reference to the value at the given index.
    pub fn get(&self, index: K) -> Option<&V> {
        self.slice.get(index.index())
    }

    /// Gets a mutable reference to the value at the given index.
    pub fn get_mut(&mut self, index: K) -> Option<&mut V> {
        self.slice.get_mut(index.index())
    }

    /// Gets the length of this slice map.
    pub fn len(&self) -> usize {
        self.slice.len()
    }

    /// Returns an iterator for this slice map.
    pub fn iter(&self) -> Iter<K, V> {
        Iter::new(self.slice.iter())
    }

    /// Returns a mutable iterator for this slice map.
    pub fn iter_mut(&mut self) -> IterMut<K, V> {
        IterMut::new(self.slice.iter_mut())
    }

    /// Gets a pointer to the `SliceMap`.
    pub fn as_ptr(&self) -> *const V {
        self as *const SliceMap<K, V> as *const V
    }

    /// Gets a mutable pointer to the `SliceMap`.
    pub fn as_mut_ptr(&mut self) -> *mut V {
        self as *mut SliceMap<K, V> as *mut V
    }
}

impl<K, V> Index<K> for SliceMap<K, V>
where
    K: TypedIndex,
{
    type Output = V;
    fn index(&self, index: K) -> &V {
        &self.slice[index.index()]
    }
}

impl<K, V> IndexMut<K> for SliceMap<K, V>
where
    K: TypedIndex,
{
    fn index_mut(&mut self, index: K) -> &mut V {
        &mut self.slice[index.index()]
    }
}
