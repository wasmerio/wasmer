use std::ops::{Index, IndexMut};
use std::ptr::NonNull;

#[derive(Copy, Clone, Debug)]
#[repr(transparent)]
pub struct UncheckedSlice<T> {
    ptr: NonNull<T>,
}

impl<T> UncheckedSlice<T> {
    #[inline]
    pub fn get_unchecked(&self, index: usize) -> &T {
        let ptr = self.ptr.as_ptr();
        unsafe { &*ptr.add(index) }
    }

    #[inline]
    pub fn get_unchecked_mut(&mut self, index: usize) -> &mut T {
        let ptr = self.ptr.as_ptr();
        unsafe { &mut *(ptr.add(index) as *mut _) }
    }

    pub unsafe fn dangling() -> UncheckedSlice<T> {
        UncheckedSlice {
            ptr: NonNull::dangling(),
        }
    }

    pub fn as_ptr(&self) -> *const T {
        self.ptr.as_ptr()
    }

    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.ptr.as_ptr()
    }
}

impl<'a, T> From<&'a [T]> for UncheckedSlice<T> {
    fn from(slice: &[T]) -> UncheckedSlice<T> {
        let ptr: NonNull<[T]> = slice.into();
        UncheckedSlice { ptr: ptr.cast() }
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct BoundedSlice<T> {
    pub data: UncheckedSlice<T>,
    pub len: usize,
}

impl<T> BoundedSlice<T> {
    pub fn get(&self, index: usize) -> Option<&T> {
        if index < self.len {
            Some(self.data.get_unchecked(index))
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index < self.len {
            Some(self.data.get_unchecked_mut(index))
        } else {
            None
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    // TODO: Needs refactor. Take LinearMemory as argument.
    //      I've tried that but it gives cryptic error.
    pub fn new(slice: &[T], size: usize) -> BoundedSlice<T> {
        BoundedSlice {
            data: slice.into(),
            len: size,
        }
    }
}

impl<T> Index<usize> for BoundedSlice<T> {
    type Output = T;
    fn index(&self, index: usize) -> &T {
        self.get(index)
            .expect(&format!("index: {} was out of bounds.", index))
    }
}

impl<T> IndexMut<usize> for BoundedSlice<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        self.get_mut(index)
            .expect(&format!("index: {} was out of bounds.", index))
    }
}

impl<'a, T> From<&'a [T]> for BoundedSlice<T> {
    fn from(slice: &[T]) -> BoundedSlice<T> {
        BoundedSlice {
            data: slice.into(),
            len: slice.len(),
        }
    }
}
