//! The structures module contains commonly used data structures.
mod boxed;
mod map;
mod slice;

pub use self::boxed::BoxedMap;
pub use self::map::{Iter, IterMut, Map};
pub use self::slice::SliceMap;

/// A trait for dealing with type-safe indices into associative data structures
/// like [`Map`]s.
///
/// Through the use of this trait, we get compile time checks that we are not
/// using the wrong type of index into our data structures.
///
/// It acts as a thin wrapper over `usize` and in most usage patterns has no
/// runtime overhead.
pub trait TypedIndex: Copy + Clone {
    /// Create a new instance of [`TypedIndex`] from a raw index value
    #[doc(hidden)]
    fn new(index: usize) -> Self;
    /// Get the raw index value from the [`TypedIndex`]
    #[doc(hidden)]
    fn index(&self) -> usize;
}
