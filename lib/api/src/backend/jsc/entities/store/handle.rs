use std::{marker::PhantomData, num::NonZeroUsize};

use wasmer_vm::StoreId;

use super::{StoreObject, StoreObjects};

/// Handle to an object managed by a context.
///
/// Internally this is just an integer index into a context. A reference to the
/// context must be passed in separately to access the actual object.
pub struct StoreHandle<T> {
    id: StoreId,
    internal: InternalStoreHandle<T>,
}

impl<T> core::cmp::PartialEq for StoreHandle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> std::hash::Hash for StoreHandle<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.internal.idx.hash(state);
    }
}

impl<T> Clone for StoreHandle<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            internal: self.internal,
        }
    }
}

impl<T: StoreObject> std::fmt::Debug for StoreHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreHandle")
            .field("id", &self.id)
            .field("internal", &self.internal.index())
            .finish()
    }
}

impl<T: StoreObject> StoreHandle<T> {
    /// Moves the given object into a context and returns a handle to it.
    pub fn new(store: &mut StoreObjects, val: T) -> Self {
        Self {
            id: store.id(),
            internal: InternalStoreHandle::new(store, val),
        }
    }

    /// Returns a reference to the object that this handle points to.
    pub fn get<'a>(&self, store: &'a StoreObjects) -> &'a T {
        assert_eq!(self.id, store.id(), "object used with the wrong context");
        self.internal.get(store)
    }

    /// Returns a mutable reference to the object that this handle points to.
    pub fn get_mut<'a>(&self, store: &'a mut StoreObjects) -> &'a mut T {
        assert_eq!(self.id, store.id(), "object used with the wrong context");
        self.internal.get_mut(store)
    }

    /// Returns the internal handle contains within this handle.
    pub fn internal_handle(&self) -> InternalStoreHandle<T> {
        self.internal
    }

    /// Returns the ID of the context associated with the handle.
    #[allow(unused)]
    pub fn store_id(&self) -> StoreId {
        self.id
    }

    /// Overrides the store id with a new ID
    #[allow(unused)]
    pub fn set_store_id(&mut self, id: StoreId) {
        self.id = id;
    }

    /// Constructs a `StoreHandle` from a `StoreId` and an `InternalStoreHandle`.
    ///
    /// # Safety
    /// Handling `InternalStoreHandle` values is unsafe because they do not track context ID.
    pub unsafe fn from_internal(id: StoreId, internal: InternalStoreHandle<T>) -> Self {
        Self { id, internal }
    }
}

/// Internal handle to an object owned by the current context.
///
/// Unlike `StoreHandle` this does not track the context ID: it is only
/// intended to be used within objects already owned by a context.
#[repr(transparent)]
pub struct InternalStoreHandle<T> {
    // Use a NonZero here to reduce the size of Option<InternalStoreHandle>.
    idx: NonZeroUsize,
    marker: PhantomData<fn() -> T>,
}

impl<T> Clone for InternalStoreHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for InternalStoreHandle<T> {}

impl<T> std::fmt::Debug for InternalStoreHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InternalStoreHandle")
            .field("idx", &self.idx)
            .finish()
    }
}
impl<T> PartialEq for InternalStoreHandle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.idx == other.idx
    }
}
impl<T> Eq for InternalStoreHandle<T> {}

impl<T: StoreObject> InternalStoreHandle<T> {
    /// Moves the given object into a context and returns a handle to it.
    pub fn new(store: &mut StoreObjects, val: T) -> Self {
        let list = T::list_mut(store);
        let idx = NonZeroUsize::new(list.len() + 1).unwrap();
        list.push(val);
        Self {
            idx,
            marker: PhantomData,
        }
    }

    /// Returns a reference to the object that this handle points to.
    pub fn get<'a>(&self, store: &'a StoreObjects) -> &'a T {
        &T::list(store)[self.idx.get() - 1]
    }

    /// Returns a mutable reference to the object that this handle points to.
    pub fn get_mut<'a>(&self, store: &'a mut StoreObjects) -> &'a mut T {
        &mut T::list_mut(store)[self.idx.get() - 1]
    }

    pub(crate) fn index(&self) -> usize {
        self.idx.get()
    }

    pub(crate) fn from_index(idx: usize) -> Option<Self> {
        NonZeroUsize::new(idx).map(|idx| Self {
            idx,
            marker: PhantomData,
        })
    }
}
