use std::any::Any;

/// Type-erased objects stored in a [`Store`].
pub type BoxStoreObject = Box<dyn Any + Send>;
/// Type-erased objects stored in a `?Send` [`Store`].
pub type LocalBoxStoreObject = Box<dyn Any>;

/// TODO document
// TODO is this name too low-level?
pub trait Upcast<T>: Sized {
    /// TODO document
    fn upcast(value: T) -> Self;
    /// TODO document
    fn downcast(self) -> Result<Box<T>, Self>;
    /// TODO document
    fn downcast_ref(&self) -> Option<&T>;
    /// TODO document
    fn downcast_mut(&mut self) -> Option<&mut T>;
}

impl<T: Send + 'static> Upcast<T> for BoxStoreObject {
    fn upcast(value: T) -> Self {
        Box::new(value) as _
    }

    fn downcast(self) -> Result<Box<T>, Self> {
        self.downcast()
    }

    fn downcast_ref(&self) -> Option<&T> {
        (**self).downcast_ref()
    }

    fn downcast_mut(&mut self) -> Option<&mut T> {
        (**self).downcast_mut()
    }
}

impl<T: 'static> Upcast<T> for LocalBoxStoreObject {
    fn upcast(value: T) -> Self {
        Box::new(value) as _
    }

    fn downcast(self) -> Result<Box<T>, Self> {
        self.downcast()
    }

    fn downcast_ref(&self) -> Option<&T> {
        (**self).downcast_ref()
    }

    fn downcast_mut(&mut self) -> Option<&mut T> {
        (**self).downcast_mut()
    }
}
