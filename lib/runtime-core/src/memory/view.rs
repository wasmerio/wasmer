use crate::types::ValueType;

use std::sync::atomic::{
    AtomicI16, AtomicI32, AtomicI64, AtomicI8, AtomicU16, AtomicU32, AtomicU64, AtomicU8,
};
use std::{cell::Cell, marker::PhantomData, ops::Deref, slice};

pub trait Atomic {
    type Output;
}
impl Atomic for i8 {
    type Output = AtomicI8;
}
impl Atomic for i16 {
    type Output = AtomicI16;
}
impl Atomic for i32 {
    type Output = AtomicI32;
}
impl Atomic for i64 {
    type Output = AtomicI64;
}
impl Atomic for u8 {
    type Output = AtomicU8;
}
impl Atomic for u16 {
    type Output = AtomicU16;
}
impl Atomic for u32 {
    type Output = AtomicU32;
}
impl Atomic for u64 {
    type Output = AtomicU64;
}
impl Atomic for f32 {
    type Output = AtomicU32;
}
impl Atomic for f64 {
    type Output = AtomicU64;
}

/// A trait that represants an atomic type.
pub trait Atomicity {}
/// Atomically.
pub struct Atomically;
impl Atomicity for Atomically {}
/// Non-atomically.
pub struct NonAtomically;
impl Atomicity for NonAtomically {}

/// A view into a memory.
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

impl<'a, T: Atomic> MemoryView<'a, T> {
    /// Get atomic access to a memory view.
    pub fn atomically(&self) -> MemoryView<'a, T::Output, Atomically> {
        MemoryView {
            ptr: self.ptr as *mut T::Output,
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

impl<'a, T> Deref for MemoryView<'a, T, Atomically> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        unsafe { slice::from_raw_parts(self.ptr as *const T, self.length) }
    }
}
