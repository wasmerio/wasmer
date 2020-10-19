//! Traits used to improve the safety / correctness of our C API implementation.
//! These types are not exposed to the C API.

/// Used to get a type when uninitialized memory is required.
///
/// This is useful for types that are not valid for arbitrary bit patterns.
/// For example types that contain `NonNull` must ensure that those fields
/// are not all-zero.
pub(crate) unsafe trait UninitDefault {
    /// Returns
    unsafe fn uninit_default(mem: *mut Self);
}

unsafe impl<T: Default> UninitDefault for T {
    unsafe fn uninit_default(mem: *mut Self) {
        *mem = Self::default();
    }
}
