use super::atomic::{Atomic, IntCast};
use crate::types::ValueType;

use std::{cell::Cell, marker::PhantomData, ops::Deref, slice};

pub trait Atomicity {}
pub struct Atomically;
impl Atomicity for Atomically {}
pub struct NonAtomically;
impl Atomicity for NonAtomically {}

pub struct MemoryView<'a, T: 'a, A = NonAtomically> {
    ptr: *mut T,
    length: usize,
    _phantom: PhantomData<(&'a [Cell<T>], A)>,
}

impl<'a, T> MemoryView<'a, T, NonAtomically>
where
    T: ValueType,
{
    pub(super) unsafe fn new(ptr: *mut T, length: u32) -> Self {
        Self {
            ptr,
            length: length as usize,
            _phantom: PhantomData,
        }
    }
}

impl<'a, T: IntCast> MemoryView<'a, T, NonAtomically> {
    pub fn atomically(&self) -> MemoryView<'a, T, Atomically> {
        MemoryView {
            ptr: self.ptr,
            length: self.length,
            _phantom: PhantomData,
        }
    }
}

impl<'a, T> Deref for MemoryView<'a, T, NonAtomically> {
    type Target = [Cell<T>];
    fn deref(&self) -> &[Cell<T>] {
        unsafe { slice::from_raw_parts(self.ptr as *const Cell<T>, self.length) }
    }
}

impl<'a, T: IntCast> Deref for MemoryView<'a, T, Atomically> {
    type Target = [Atomic<T>];
    fn deref(&self) -> &[Atomic<T>] {
        unsafe { slice::from_raw_parts(self.ptr as *const Atomic<T>, self.length) }
    }
}
