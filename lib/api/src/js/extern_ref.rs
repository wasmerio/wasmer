use std::any::Any;
use super::store::{AsStoreMut, AsStoreRef};

#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct ExternRef {
    inner: Box<dyn Any>,
}

impl ExternRef {
    /// Make a new extern reference
    pub fn new<T>(_store: &mut impl AsStoreMut, value: T) -> Self {
        Self {
            inner: Box::new(value),
        }
    }

    /// Try to downcast to the given value.
    pub fn downcast<'a, T>(&self, _store: &'a impl AsStoreRef) -> Option<&'a T>
    where T: Any
    {
        self.inner.downcast().ok()
    }
    
    pub unsafe fn from_raw(address_js: f64) -> Self { 
        Box::from_raw(ptr as usize)
    }

    pub fn to_raw(&self) -> f64 {
        self.inner.as_ref() as usize as f64
    }
}