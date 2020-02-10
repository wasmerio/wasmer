use super::{BoxedMap, SliceMap, TypedIndex};
use std::{
    iter::{self, Extend, FromIterator},
    marker::PhantomData,
    mem,
    ops::{Deref, DerefMut},
    slice, vec,
};

/// Dense item map
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Map<K, V>
where
    K: TypedIndex,
{
    elems: Vec<V>,
    _marker: PhantomData<K>,
}

impl<K, V> Map<K, V>
where
    K: TypedIndex,
{
    /// Creates a new `Map`.
    pub fn new() -> Self {
        Self {
            elems: Vec::new(),
            _marker: PhantomData,
        }
    }

    /// Creates a new empty `Map` with the given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            elems: Vec::with_capacity(capacity),
            _marker: PhantomData,
        }
    }

    /// Clears the map. Keeps the allocated memory for future use.
    pub fn clear(&mut self) {
        self.elems.clear();
    }

    /// Returns the size of this map.
    pub fn len(&self) -> usize {
        self.elems.len()
    }

    /// Returns true if this map is empty.
    pub fn is_empty(&self) -> bool {
        self.elems.is_empty()
    }

    /// Adds a new value to this map.
    pub fn push(&mut self, value: V) -> K {
        let len = self.len();
        self.elems.push(value);
        K::new(len)
    }

    /// Returns the next index into the map.
    pub fn next_index(&self) -> K {
        K::new(self.len())
    }

    /// Reserves the given size.
    pub fn reserve_exact(&mut self, size: usize) {
        self.elems.reserve_exact(size);
    }

    /// Convert this into a `BoxedMap`.
    pub fn into_boxed_map(self) -> BoxedMap<K, V> {
        BoxedMap::new(self.elems.into_boxed_slice())
    }

    /// Convert this into a `Vec`.
    pub fn into_vec(self) -> Vec<V> {
        self.elems
    }
}

impl<K, V> Map<K, V>
where
    K: TypedIndex,
    V: Clone,
{
    /// Resize this map to the given new length and value.
    pub fn resize(&mut self, new_len: usize, value: V) {
        self.elems.resize(new_len, value);
    }
}

impl<K, V> Extend<V> for Map<K, V>
where
    K: TypedIndex,
{
    fn extend<I: IntoIterator<Item = V>>(&mut self, iter: I) {
        self.elems.extend(iter);
    }
}

impl<K, V> FromIterator<V> for Map<K, V>
where
    K: TypedIndex,
{
    fn from_iter<I: IntoIterator<Item = V>>(iter: I) -> Self {
        let elems: Vec<V> = iter.into_iter().collect();
        Self {
            elems,
            _marker: PhantomData,
        }
    }
}

impl<K, V> Deref for Map<K, V>
where
    K: TypedIndex,
{
    type Target = SliceMap<K, V>;
    fn deref(&self) -> &SliceMap<K, V> {
        unsafe { mem::transmute::<&[V], _>(self.elems.as_slice()) }
    }
}

impl<K, V> DerefMut for Map<K, V>
where
    K: TypedIndex,
{
    fn deref_mut(&mut self) -> &mut SliceMap<K, V> {
        unsafe { mem::transmute::<&mut [V], _>(self.elems.as_mut_slice()) }
    }
}

pub struct IntoIter<K, V>
where
    K: TypedIndex,
{
    enumerated: iter::Enumerate<vec::IntoIter<V>>,
    _marker: PhantomData<K>,
}

impl<K, V> IntoIter<K, V>
where
    K: TypedIndex,
{
    pub(in crate::structures) fn new(into_iter: vec::IntoIter<V>) -> Self {
        Self {
            enumerated: into_iter.enumerate(),
            _marker: PhantomData,
        }
    }
}

impl<K, V> Iterator for IntoIter<K, V>
where
    K: TypedIndex,
{
    type Item = (K, V);

    fn next(&mut self) -> Option<(K, V)> {
        self.enumerated.next().map(|(i, v)| (K::new(i), v))
    }
}

impl<K, V> IntoIterator for Map<K, V>
where
    K: TypedIndex,
{
    type Item = (K, V);
    type IntoIter = IntoIter<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter::new(self.elems.into_iter())
    }
}

impl<'a, K, V> IntoIterator for &'a Map<K, V>
where
    K: TypedIndex,
{
    type Item = (K, &'a V);
    type IntoIter = Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        Iter::new(self.elems.iter())
    }
}

impl<'a, K, V> IntoIterator for &'a mut Map<K, V>
where
    K: TypedIndex,
{
    type Item = (K, &'a mut V);
    type IntoIter = IterMut<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        IterMut::new(self.elems.iter_mut())
    }
}

/// Iterator for a `Map`.
pub struct Iter<'a, K: TypedIndex, V: 'a> {
    enumerated: iter::Enumerate<slice::Iter<'a, V>>,
    _marker: PhantomData<K>,
}

impl<'a, K: TypedIndex, V: 'a> Iter<'a, K, V> {
    pub(in crate::structures) fn new(iter: slice::Iter<'a, V>) -> Self {
        Self {
            enumerated: iter.enumerate(),
            _marker: PhantomData,
        }
    }
}

impl<'a, K: TypedIndex, V: 'a> Iterator for Iter<'a, K, V> {
    type Item = (K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        self.enumerated.next().map(|(i, v)| (K::new(i), v))
    }
}

/// Mutable iterator for a `Map`.
pub struct IterMut<'a, K: TypedIndex, V: 'a> {
    enumerated: iter::Enumerate<slice::IterMut<'a, V>>,
    _marker: PhantomData<K>,
}

impl<'a, K: TypedIndex, V: 'a> IterMut<'a, K, V> {
    pub(in crate::structures) fn new(iter: slice::IterMut<'a, V>) -> Self {
        Self {
            enumerated: iter.enumerate(),
            _marker: PhantomData,
        }
    }
}

impl<'a, K: TypedIndex, V: 'a> Iterator for IterMut<'a, K, V> {
    type Item = (K, &'a mut V);

    fn next(&mut self) -> Option<Self::Item> {
        self.enumerated.next().map(|(i, v)| (K::new(i), v))
    }
}
