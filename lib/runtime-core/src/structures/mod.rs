//! The structures module contains commonly used data structures.
mod boxed;
mod map;
mod slice;

pub use self::boxed::BoxedMap;
pub use self::map::{Iter, IterMut, Map};
pub use self::slice::SliceMap;

/// Represents a typed index.
pub trait TypedIndex: Copy + Clone {
    #[doc(hidden)]
    fn new(index: usize) -> Self;
    #[doc(hidden)]
    fn index(&self) -> usize;
}
