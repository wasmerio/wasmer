use core::cmp::Ordering;
use core::fmt::{self, Debug, Display};
use core::mem;
use core::ops::{Deref, DerefMut};
use core::ptr;
use std::marker::PhantomData;

use js_sys::Uint8Array;
use wasm_bindgen::JsValue;

/// A mutable memory location.
///
/// # Examples
///
/// In this example, you can see that `WasmCell<T>` enables mutation inside an
/// immutable struct. In other words, it enables "interior mutability".
///
/// ```
/// use wasmer::WasmCell;
///
/// struct SomeStruct {
///     regular_field: u8,
///     special_field: WasmCell<u8>,
/// }
///
/// let my_struct = SomeStruct {
///     regular_field: 0,
///     special_field: WasmCell::new(1),
/// };
///
/// let new_value = 100;
///
/// // ERROR: `my_struct` is immutable
/// // my_struct.regular_field = new_value;
///
/// // WORKS: although `my_struct` is immutable, `special_field` is a `WasmCell`,
/// // which can always be mutated
/// my_struct.special_field.set(new_value);
/// assert_eq!(my_struct.special_field.get(), new_value);
/// ```
///
/// See the [module-level documentation](self) for more.
#[derive(Clone)]
pub struct WasmCell<T: ?Sized> {
    pub(crate) memory: Uint8Array,
    phantom: PhantomData<T>,
}

unsafe impl<T: ?Sized> Send for WasmCell<T> where T: Send {}

unsafe impl<T: ?Sized> Sync for WasmCell<T> {}

// impl<T: Copy> Clone for WasmCell<T> {
//     #[inline]
//     fn clone(&self) -> WasmCell<T> {
//         WasmCell::new(self.get())
//     }
// }

impl<T: Default> Default for WasmCell<T> {
    /// Creates a `WasmCell<T>`, with the `Default` value for T.
    #[inline]
    fn default() -> WasmCell<T> {
        unimplemented!()
        // WasmCell::new(Default::default())
    }
}

impl<T: PartialEq + Copy> PartialEq for WasmCell<T> {
    #[inline]
    fn eq(&self, other: &WasmCell<T>) -> bool {
        true
    }
}

impl<T: Eq + Copy> Eq for WasmCell<T> {}

impl<T: PartialOrd + Copy> PartialOrd for WasmCell<T> {
    #[inline]
    fn partial_cmp(&self, other: &WasmCell<T>) -> Option<Ordering> {
        self.get().partial_cmp(&other.get())
    }

    #[inline]
    fn lt(&self, other: &WasmCell<T>) -> bool {
        self.get() < other.get()
    }

    #[inline]
    fn le(&self, other: &WasmCell<T>) -> bool {
        self.get() <= other.get()
    }

    #[inline]
    fn gt(&self, other: &WasmCell<T>) -> bool {
        self.get() > other.get()
    }

    #[inline]
    fn ge(&self, other: &WasmCell<T>) -> bool {
        self.get() >= other.get()
    }
}

impl<T: Ord + Copy> Ord for WasmCell<T> {
    #[inline]
    fn cmp(&self, other: &WasmCell<T>) -> Ordering {
        self.get().cmp(&other.get())
    }
}

impl<T> From<T> for WasmCell<T> {
    fn from(t: T) -> WasmCell<T> {
        unimplemented!();
        // WasmCell::new(t)
    }
}

impl<T> WasmCell<T> {
    /// Creates a new `WasmCell` containing the given value.
    ///
    /// # Examples
    ///
    /// ```
    /// use wasmer::WasmCell;
    ///
    /// let c = WasmCell::new(5);
    /// ```
    #[inline]
    pub const fn new(memory: Uint8Array) -> WasmCell<T> {
        // WasmCell { value: UnsafeWasmCell::new(value) }
        WasmCell {
            memory,
            phantom: PhantomData,
        }
    }

    /// Swaps the values of two WasmCells.
    /// Difference with `std::mem::swap` is that this function doesn't require `&mut` reference.
    ///
    /// # Examples
    ///
    /// ```
    /// use wasmer::WasmCell;
    ///
    /// let c1 = WasmCell::new(5i32);
    /// let c2 = WasmCell::new(10i32);
    /// c1.swap(&c2);
    /// assert_eq!(10, c1.get());
    /// assert_eq!(5, c2.get());
    /// ```
    #[inline]
    pub fn swap(&self, other: &Self) {
        unimplemented!();
        // if ptr::eq(self, other) {
        //     return;
        // }
        // // SAFETY: This can be risky if called from separate threads, but `WasmCell`
        // // is `!Sync` so this won't happen. This also won't invalidate any
        // // pointers since `WasmCell` makes sure nothing else will be pointing into
        // // either of these `WasmCell`s.
        // unsafe {
        //     ptr::swap(self.value.get(), other.value.get());
        // }
    }

    /// Replaces the contained value with `val`, and returns the old contained value.
    ///
    /// # Examples
    ///
    /// ```
    /// use wasmer::WasmCell;
    ///
    /// let cell = WasmCell::new(5);
    /// assert_eq!(cell.get(), 5);
    /// assert_eq!(cell.replace(10), 5);
    /// assert_eq!(cell.get(), 10);
    /// ```
    pub fn replace(&self, val: T) -> T {
        unimplemented!();
        // SAFETY: This can cause data races if called from a separate thread,
        // but `WasmCell` is `!Sync` so this won't happen.
        // mem::replace(unsafe { &mut *self.value.get() }, val)
    }

    // /// Unwraps the value.
    // ///
    // /// # Examples
    // ///
    // /// ```
    // /// use wasmer::WasmCell;
    // ///
    // /// let c = WasmCell::new(5);
    // /// let five = c.into_inner();
    // ///
    // /// assert_eq!(five, 5);
    // /// ```
    // pub const fn into_inner(self) -> T {
    //     // This will get the item out of the MemoryView and into
    //     // Rust memory allocator
    //     unimplemented!()
    //     // self.get()
    // }
}

impl<T: Copy> WasmCell<T> {
    /// Returns a copy of the contained value.
    ///
    /// # Examples
    ///
    /// ```
    /// use wasmer::WasmCell;
    ///
    /// let c = WasmCell::new(5);
    ///
    /// let five = c.get();
    /// ```
    #[inline]
    pub fn get(&self) -> T {
        let vec = self.memory.to_vec();
        unsafe { *(vec.as_slice().as_ptr() as *const T) }
        // unimplemented!();
    }

    /// Updates the contained value using a function and returns the new value.
    ///
    /// # Examples
    ///
    /// ```
    /// #![feature(cell_update)]
    ///
    /// use wasmer::WasmCell;
    ///
    /// let c = WasmCell::new(5);
    /// let new = c.update(|x| x + 1);
    ///
    /// assert_eq!(new, 6);
    /// assert_eq!(c.get(), 6);
    /// ```
    #[inline]
    pub fn update<F>(&self, f: F) -> T
    where
        F: FnOnce(T) -> T,
    {
        let old = self.get();
        let new = f(old);
        self.set(new);
        new
    }
}

impl<T: Sized> WasmCell<T> {
    /// Sets the contained value.
    ///
    /// # Examples
    ///
    /// ```
    /// use wasmer::WasmCell;
    ///
    /// let c = WasmCell::new(5);
    ///
    /// c.set(10);
    /// ```
    #[inline]
    pub fn set(&self, val: T) {
        let size = std::mem::size_of::<T>();
        let ptr = &val as *const T as *const u8;
        let slice = unsafe { std::slice::from_raw_parts(ptr, size) };
        self.memory.copy_from(slice);
        // p.as_ptr();
        // let ptr = (&val) as usize;
        // unimplemented!();
        // let old = self.replace(val);
        // drop(old);
    }
}
//     /// Returns a raw pointer to the underlying data in this cell.
//     ///
//     /// # Examples
//     ///
//     /// ```
//     /// use wasmer::WasmCell;
//     ///
//     /// let c = WasmCell::new(5);
//     ///
//     /// let ptr = c.as_ptr();
//     /// ```
//     #[inline]
//     pub const fn as_ptr(&self) -> *mut T {
//         self.value.get()
//     }

//     /// Returns a mutable reference to the underlying data.
//     ///
//     /// This call borrows `WasmCell` mutably (at compile-time) which guarantees
//     /// that we possess the only reference.
//     ///
//     /// # Examples
//     ///
//     /// ```
//     /// use wasmer::WasmCell;
//     ///
//     /// let mut c = WasmCell::new(5);
//     /// *c.get_mut() += 1;
//     ///
//     /// assert_eq!(c.get(), 6);
//     /// ```
//     #[inline]
//     pub fn get_mut(&mut self) -> &mut T {
//         self.value.get_mut()
//     }

//     /// Returns a `&WasmCell<T>` from a `&mut T`
//     ///
//     /// # Examples
//     ///
//     /// ```
//     /// use wasmer::WasmCell;
//     ///
//     /// let slice: &mut [i32] = &mut [1, 2, 3];
//     /// let cell_slice: &WasmCell<[i32]> = WasmCell::from_mut(slice);
//     /// let slice_cell: &[WasmCell<i32>] = cell_slice.as_slice_of_cells();
//     ///
//     /// assert_eq!(slice_cell.len(), 3);
//     /// ```
//     #[inline]
//     pub fn from_mut(t: &mut T) -> &WasmCell<T> {
//         // SAFETY: `&mut` ensures unique access.
//         unsafe { &*(t as *mut T as *const WasmCell<T>) }
//     }
// }

// impl<T: Default> WasmCell<T> {
//     /// Takes the value of the cell, leaving `Default::default()` in its place.
//     ///
//     /// # Examples
//     ///
//     /// ```
//     /// use wasmer::WasmCell;
//     ///
//     /// let c = WasmCell::new(5);
//     /// let five = c.take();
//     ///
//     /// assert_eq!(five, 5);
//     /// assert_eq!(c.into_inner(), 0);
//     /// ```
//     pub fn take(&self) -> T {
//         self.replace(Default::default())
//     }
// }

impl<T> WasmCell<[T]> {
    /// Returns a `&[WasmCell<T>]` from a `&WasmCell<[T]>`
    ///
    /// # Examples
    ///
    /// ```
    /// use wasmer::WasmCell;
    ///
    /// let slice: &mut [i32] = &mut [1, 2, 3];
    /// let cell_slice: &WasmCell<[i32]> = WasmCell::from_mut(slice);
    /// let slice_cell: &[WasmCell<i32>] = cell_slice.as_slice_of_cells();
    ///
    /// assert_eq!(slice_cell.len(), 3);
    /// ```
    pub fn as_slice_of_cells(&self) -> &[WasmCell<T>] {
        unimplemented!();
        // SAFETY: `WasmCell<T>` has the same memory layout as `T`.
        // unsafe { &*(self as *const WasmCell<[T]> as *const [WasmCell<T>]) }
    }
}
