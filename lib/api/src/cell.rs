pub use std::cell::Cell;

use core::cmp::Ordering;
use core::fmt::{self, Debug};
use std::fmt::Pointer;

/// A mutable Wasm-memory location.
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
#[repr(transparent)]
pub struct WasmCell<'a, T: ?Sized> {
    inner: &'a Cell<T>,
}

unsafe impl<T: ?Sized> Send for WasmCell<'_, T> where T: Send {}

unsafe impl<T: ?Sized> Sync for WasmCell<'_, T> {}

impl<'a, T: Copy> Clone for WasmCell<'a, T> {
    #[inline]
    fn clone(&self) -> WasmCell<'a, T> {
        WasmCell { inner: self.inner }
    }
}

impl<T: PartialEq + Copy> PartialEq for WasmCell<'_, T> {
    #[inline]
    fn eq(&self, other: &WasmCell<T>) -> bool {
        self.inner.eq(&other.inner)
    }
}

impl<T: Eq + Copy> Eq for WasmCell<'_, T> {}

impl<T: PartialOrd + Copy> PartialOrd for WasmCell<'_, T> {
    #[inline]
    fn partial_cmp(&self, other: &WasmCell<T>) -> Option<Ordering> {
        self.inner.partial_cmp(&other.inner)
    }

    #[inline]
    fn lt(&self, other: &WasmCell<T>) -> bool {
        self.inner < other.inner
    }

    #[inline]
    fn le(&self, other: &WasmCell<T>) -> bool {
        self.inner <= other.inner
    }

    #[inline]
    fn gt(&self, other: &WasmCell<T>) -> bool {
        self.inner > other.inner
    }

    #[inline]
    fn ge(&self, other: &WasmCell<T>) -> bool {
        self.inner >= other.inner
    }
}

impl<T: Ord + Copy> Ord for WasmCell<'_, T> {
    #[inline]
    fn cmp(&self, other: &WasmCell<T>) -> Ordering {
        self.inner.cmp(&other.inner)
    }
}

impl<'a, T> WasmCell<'a, T> {
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
    pub const fn new(cell: &'a Cell<T>) -> WasmCell<'a, T> {
        WasmCell { inner: cell }
    }
}

impl<'a, T: Copy> WasmCell<'a, T> {
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
        self.inner.get()
    }

    /// Get an unsafe mutable pointer to the inner item
    /// in the Cell.
    pub unsafe fn get_mut(&self) -> &'a mut T {
        &mut *self.inner.as_ptr()
    }
}

impl<T: Debug> Debug for WasmCell<'_, T> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}

impl<T: Sized> WasmCell<'_, T> {
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
        self.inner.set(val);
    }
}
