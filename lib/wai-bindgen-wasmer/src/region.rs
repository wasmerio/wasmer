use crate::rt::RawMem;
use crate::{Endian, GuestError, Le};
use std::collections::HashSet;
use std::convert::TryInto;
use std::marker;
use std::mem;
use wasmer::RuntimeError;

// This is a pretty naive way to account for borrows. This datastructure
// could be made a lot more efficient with some effort.
pub struct BorrowChecker<'a> {
    /// Maps from handle to region borrowed. A HashMap is probably not ideal
    /// for this but it works. It would be more efficient if we could
    /// check `is_borrowed` without an O(n) iteration, by organizing borrows
    /// by an ordering of Region.
    shared_borrows: HashSet<Region>,
    mut_borrows: HashSet<Region>,
    _marker: marker::PhantomData<&'a mut [u8]>,
    ptr: *mut u8,
    len: usize,
}

// These are not automatically implemented with our storage of `*mut u8`, so we
// need to manually declare that this type is threadsafe.
unsafe impl Send for BorrowChecker<'_> {}
unsafe impl Sync for BorrowChecker<'_> {}

fn to_error(err: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::new(err.to_string())
}

impl<'a> BorrowChecker<'a> {
    pub fn new(data: &'a mut [u8]) -> BorrowChecker<'a> {
        BorrowChecker {
            ptr: data.as_mut_ptr(),
            len: data.len(),
            shared_borrows: Default::default(),
            mut_borrows: Default::default(),
            _marker: marker::PhantomData,
        }
    }

    pub fn slice<T: AllBytesValid>(&mut self, ptr: i32, len: i32) -> Result<&'a [T], RuntimeError> {
        let (ret, r) = self.get_slice(ptr, len)?;
        // SAFETY: We're promoting the valid lifetime of `ret` from a temporary
        // borrow on `self` to `'a` on this `BorrowChecker`. At the same time
        // we're recording that this is a persistent shared borrow (until this
        // borrow checker is deleted), which disallows future mutable borrows
        // of the same data.
        let ret = unsafe { &*(ret as *const [T]) };
        self.shared_borrows.insert(r);
        Ok(ret)
    }

    pub fn slice_mut<T: AllBytesValid>(
        &mut self,
        ptr: i32,
        len: i32,
    ) -> Result<&'a mut [T], RuntimeError> {
        let (ret, r) = self.get_slice_mut(ptr, len)?;
        // SAFETY: see `slice` for how we're extending the lifetime by
        // recording the borrow here. Note that the `mut_borrows` list is
        // checked on both shared and mutable borrows in the future since a
        // mutable borrow can't alias with anything.
        let ret = unsafe { &mut *(ret as *mut [T]) };
        self.mut_borrows.insert(r);
        Ok(ret)
    }

    fn get_slice<T: AllBytesValid>(
        &self,
        ptr: i32,
        len: i32,
    ) -> Result<(&[T], Region), RuntimeError> {
        let r = self.region::<T>(ptr, len)?;
        if self.is_mut_borrowed(r) {
            Err(to_error(GuestError::PtrBorrowed(r)))
        } else {
            Ok((
                // SAFETY: invariants to uphold:
                //
                // * The lifetime of the input is valid for the lifetime of the
                //   output. In this case we're threading through the lifetime
                //   of `&self` to the output.
                // * The actual output is valid, which is guaranteed with the
                //   `AllBytesValid` bound.
                // * We uphold Rust's borrowing guarantees, namely that this
                //   borrow we're returning isn't overlapping with any mutable
                //   borrows.
                // * The region `r` we're returning accurately describes the
                //   slice we're returning in wasm linear memory.
                unsafe {
                    std::slice::from_raw_parts(
                        self.ptr.add(r.start as usize) as *const T,
                        len as usize,
                    )
                },
                r,
            ))
        }
    }

    fn get_slice_mut<T>(&mut self, ptr: i32, len: i32) -> Result<(&mut [T], Region), RuntimeError> {
        let r = self.region::<T>(ptr, len)?;
        if self.is_mut_borrowed(r) || self.is_shared_borrowed(r) {
            Err(to_error(GuestError::PtrBorrowed(r)))
        } else {
            Ok((
                // SAFETY: same as `get_slice`, except for that we're threading
                // through `&mut` properties as well.
                unsafe {
                    std::slice::from_raw_parts_mut(
                        self.ptr.add(r.start as usize) as *mut T,
                        len as usize,
                    )
                },
                r,
            ))
        }
    }

    fn region<T>(&self, ptr: i32, len: i32) -> Result<Region, RuntimeError> {
        assert_eq!(std::mem::align_of::<T>(), 1);
        let r = Region {
            start: ptr as u32,
            len: (len as u32)
                .checked_mul(mem::size_of::<T>() as u32)
                .ok_or_else(|| to_error(GuestError::PtrOverflow))?,
        };
        self.validate_contains(&r)?;
        Ok(r)
    }

    pub fn slice_str(&mut self, ptr: i32, len: i32) -> Result<&'a str, RuntimeError> {
        let bytes = self.slice(ptr, len)?;
        std::str::from_utf8(bytes).map_err(to_error)
    }

    fn validate_contains(&self, region: &Region) -> Result<(), RuntimeError> {
        let end = region
            .start
            .checked_add(region.len)
            .ok_or_else(|| to_error(GuestError::PtrOverflow))? as usize;
        if end <= self.len {
            Ok(())
        } else {
            Err(to_error(GuestError::PtrOutOfBounds(*region)))
        }
    }

    fn is_shared_borrowed(&self, r: Region) -> bool {
        self.shared_borrows.iter().any(|b| b.overlaps(r))
    }

    fn is_mut_borrowed(&self, r: Region) -> bool {
        self.mut_borrows.iter().any(|b| b.overlaps(r))
    }

    pub fn raw(&self) -> *mut [u8] {
        std::ptr::slice_from_raw_parts_mut(self.ptr, self.len)
    }
}

impl RawMem for BorrowChecker<'_> {
    fn store<T: Endian>(&mut self, offset: i32, val: T) -> Result<(), RuntimeError> {
        let (slice, _) = self.get_slice_mut::<Le<T>>(offset, 1)?;
        slice[0].set(val);
        Ok(())
    }

    fn store_many<T: Endian>(&mut self, offset: i32, val: &[T]) -> Result<(), RuntimeError> {
        let (slice, _) = self.get_slice_mut::<Le<T>>(
            offset,
            val.len()
                .try_into()
                .map_err(|_| to_error(GuestError::PtrOverflow))?,
        )?;
        for (slot, val) in slice.iter_mut().zip(val) {
            slot.set(*val);
        }
        Ok(())
    }

    fn load<T: Endian>(&self, offset: i32) -> Result<T, RuntimeError> {
        let (slice, _) = self.get_slice::<Le<T>>(offset, 1)?;
        Ok(slice[0].get())
    }
}

/// Unsafe trait representing types where every byte pattern is valid for their
/// representation.
///
/// This is the set of types which wasmer can have a raw pointer to for
/// values which reside in wasm linear memory.
///
/// # Safety
///
/// TODO: add safety docs.
///
pub unsafe trait AllBytesValid {}

unsafe impl AllBytesValid for u8 {}
unsafe impl AllBytesValid for u16 {}
unsafe impl AllBytesValid for u32 {}
unsafe impl AllBytesValid for u64 {}
unsafe impl AllBytesValid for i8 {}
unsafe impl AllBytesValid for i16 {}
unsafe impl AllBytesValid for i32 {}
unsafe impl AllBytesValid for i64 {}
unsafe impl AllBytesValid for f32 {}
unsafe impl AllBytesValid for f64 {}

macro_rules! tuples {
    ($(($($t:ident)*))*) => ($(
        unsafe impl <$($t:AllBytesValid,)*> AllBytesValid for ($($t,)*) {}
    )*)
}

tuples! {
    ()
    (T1)
    (T1 T2)
    (T1 T2 T3)
    (T1 T2 T3 T4)
    (T1 T2 T3 T4 T5)
    (T1 T2 T3 T4 T5 T6)
    (T1 T2 T3 T4 T5 T6 T7)
    (T1 T2 T3 T4 T5 T6 T7 T8)
    (T1 T2 T3 T4 T5 T6 T7 T8 T9)
    (T1 T2 T3 T4 T5 T6 T7 T8 T9 T10)
}

/// Represents a contiguous region in memory.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct Region {
    pub start: u32,
    pub len: u32,
}

impl Region {
    /// Checks if this `Region` overlaps with `rhs` `Region`.
    fn overlaps(&self, rhs: Region) -> bool {
        // Zero-length regions can never overlap!
        if self.len == 0 || rhs.len == 0 {
            return false;
        }

        let self_start = self.start as u64;
        let self_end = self_start + (self.len - 1) as u64;

        let rhs_start = rhs.start as u64;
        let rhs_end = rhs_start + (rhs.len - 1) as u64;

        if self_start <= rhs_start {
            self_end >= rhs_start
        } else {
            rhs_end >= self_start
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn nonoverlapping() {
        let mut bytes = [0; 100];
        let mut bc = BorrowChecker::new(&mut bytes);
        bc.slice::<u8>(0, 10).unwrap();
        bc.slice::<u8>(10, 10).unwrap();

        let mut bc = BorrowChecker::new(&mut bytes);
        bc.slice::<u8>(10, 10).unwrap();
        bc.slice::<u8>(0, 10).unwrap();

        let mut bc = BorrowChecker::new(&mut bytes);
        bc.slice_mut::<u8>(0, 10).unwrap();
        bc.slice_mut::<u8>(10, 10).unwrap();

        let mut bc = BorrowChecker::new(&mut bytes);
        bc.slice_mut::<u8>(10, 10).unwrap();
        bc.slice_mut::<u8>(0, 10).unwrap();
    }

    #[test]
    fn overlapping() {
        let mut bytes = [0; 100];
        let mut bc = BorrowChecker::new(&mut bytes);
        bc.slice::<u8>(0, 10).unwrap();
        bc.slice_mut::<u8>(9, 10).unwrap_err();
        bc.slice::<u8>(9, 10).unwrap();

        let mut bc = BorrowChecker::new(&mut bytes);
        bc.slice::<u8>(0, 10).unwrap();
        bc.slice_mut::<u8>(2, 5).unwrap_err();
        bc.slice::<u8>(2, 5).unwrap();

        let mut bc = BorrowChecker::new(&mut bytes);
        bc.slice::<u8>(9, 10).unwrap();
        bc.slice_mut::<u8>(0, 10).unwrap_err();
        bc.slice::<u8>(0, 10).unwrap();

        let mut bc = BorrowChecker::new(&mut bytes);
        bc.slice::<u8>(2, 5).unwrap();
        bc.slice_mut::<u8>(0, 10).unwrap_err();
        bc.slice::<u8>(0, 10).unwrap();

        let mut bc = BorrowChecker::new(&mut bytes);
        bc.slice::<u8>(2, 5).unwrap();
        bc.slice::<u8>(10, 5).unwrap();
        bc.slice::<u8>(15, 5).unwrap();
        bc.slice_mut::<u8>(0, 10).unwrap_err();
        bc.slice::<u8>(0, 10).unwrap();
    }

    #[test]
    fn zero_length() {
        let mut bytes = [0; 100];
        let mut bc = BorrowChecker::new(&mut bytes);
        bc.slice_mut::<u8>(0, 0).unwrap();
        bc.slice_mut::<u8>(0, 0).unwrap();
        bc.slice::<u8>(0, 1).unwrap();
    }
}
