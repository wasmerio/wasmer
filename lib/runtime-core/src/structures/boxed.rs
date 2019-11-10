use super::{SliceMap, TypedIndex};
use std::{
    marker::PhantomData,
    mem,
    ops::{Deref, DerefMut},
};

/// Boxed map.
#[derive(Debug, Clone)]
pub struct BoxedMap<K, V>
where
    K: TypedIndex,
{
    elems: Box<[V]>,
    _marker: PhantomData<K>,
}

impl<K, V> BoxedMap<K, V>
where
    K: TypedIndex,
{
    pub(in crate::structures) fn new(elems: Box<[V]>) -> Self {
        Self {
            elems,
            _marker: PhantomData,
        }
    }
}

impl<K, V> Deref for BoxedMap<K, V>
where
    K: TypedIndex,
{
    type Target = SliceMap<K, V>;
    fn deref(&self) -> &SliceMap<K, V> {
        unsafe { mem::transmute::<&[V], _>(&*self.elems) }
    }
}

impl<K, V> DerefMut for BoxedMap<K, V>
where
    K: TypedIndex,
{
    fn deref_mut(&mut self) -> &mut SliceMap<K, V> {
        unsafe { mem::transmute::<&mut [V], _>(&mut *self.elems) }
    }
}
