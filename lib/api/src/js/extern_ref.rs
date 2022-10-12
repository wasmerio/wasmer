use std::any::Any;
use super::store::{AsStoreMut, AsStoreRef};
use crate::js::store::StoreId;

/// JS ExternRef, internally casted as a `Box<dyn Any>`
#[derive(Debug)]
pub struct ExternRef {
    inner: Box<dyn Any>,
    store_id: StoreId,
}

impl ExternRef {
    /// Make a new extern reference
    pub fn new<T: 'static>(store: &mut impl AsStoreMut, value: T) -> Self {
        Self {
            inner: Box::new(value),
            store_id: store.as_store_ref().objects().id(),
        }
    }

    /// Checks whether this object came from the store
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.store_id == store.as_store_ref().objects().id()
    }

    /// Try to downcast to the given value.
    pub fn downcast<'a, T>(&'a self, _store: &impl AsStoreRef) -> Option<&'a T>
    where T: Any + Sized + 'static
    {
        self.inner.downcast_ref::<T>()
    }
}