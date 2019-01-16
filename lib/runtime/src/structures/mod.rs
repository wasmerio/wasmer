mod boxed;
mod map;
mod slice;

pub use self::boxed::BoxedMap;
pub use self::map::{Iter, IterMut, Map};
pub use self::slice::SliceMap;

pub trait TypedIndex {
    fn new(index: usize) -> Self;
    fn index(&self) -> usize;
}
