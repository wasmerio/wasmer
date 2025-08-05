use std::{any::Any, fmt, num::NonZeroUsize};

use crate::StoreId;

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


/// Trait to represent an object managed by a context. This is implemented on
/// the VM types managed by the context.
pub trait ObjectStore<K> {
    /// The type of data this type refers to in the store.
    type Value;

    /// Get the unique ID of the store.
    fn store_id(&self) -> StoreId;

    /// List the objects in the store.
    fn list(&self) -> &Vec<Self::Value>;

    /// List the objects in the store, mutably.
    fn list_mut(&mut self) -> &mut Vec<Self::Value>;

    /// Insert an object into the store, returning a new handle to it.
    fn insert(&mut self, value: Self::Value) -> StoreHandle<K> {
        StoreHandle {
            id: self.store_id(),
            internal: InternalStoreHandle::new(self, value),
        }
    }
}

/// TODO document
pub trait StoreObject<Store> {
    /// TODO document
    type Value;
}

impl<T, Store: ObjectStore<T>> StoreObject<Store> for T {
    type Value = Store::Value;
}

/// Implement the `ObjectStore<K>` trait for a set of `K`s by
/// accessing fields of the appropriate types.
#[macro_export]
macro_rules! impl_object_store {
    (@@, $Self:ident, [ $(<$($params:ident),*>)? ], $Trait:path, $field:ident, $Value:ty) => {
        impl $(<$($params),*>)? $Trait for $Self $(<$($params,)*>)? {
            type Value = $Value;

            fn store_id(&self) -> StoreId {
                self.id
            }

            fn list(&self) -> &Vec<Self::Value> {
                &self.$field
            }

            fn list_mut(&mut self) -> &mut Vec<Self::Value> {
                &mut self.$field
            }
        }
    };

    (@ $Self:ident $Self_params:tt $($field:ident : $Value:ident $(<$($Value_params:ident),*>)? ,)*) => {
        $($crate::impl_object_store!(@@, $Self, $Self_params, $crate::ObjectStore<$Value>, $field, $Value $(<$($Value_params),*>)?);)*
    };
    ($Self:ident $(<$($Self_params:ident),*>)? { $($field:ident : $Value:ident $(<$($Value_params:ident),*>)? ,)* }) => {
        $crate::impl_object_store!(@ $Self [ $(<$($Self_params),*>)? ] $($field: $Value $(<$($Value_params),*>)?, )*);
    };
}

/// Handle to an object managed by a context.
///
/// Internally this is just an integer index into a context. A reference to the
/// context must be passed in separately to access the actual object.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct StoreHandle<T> {
    id: crate::StoreId,
    internal: InternalStoreHandle<T>,
}

impl<T> Clone for StoreHandle<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            internal: self.internal,
        }
    }
}

impl<T> std::hash::Hash for StoreHandle<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.internal.idx.hash(state);
    }
}

impl<T> fmt::Debug for StoreHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoreHandle")
            .field("id", &self.id)
            .field("internal", &self.internal.index())
            .finish()
    }
}

impl<T> PartialEq for StoreHandle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.internal == other.internal
    }
}

impl<T> Eq for StoreHandle<T> {}

impl<T> StoreHandle<T> {
    /// Returns a reference to the object that this handle points to.
    pub fn get<'a, S: ObjectStore<T>>(&self, ctx: &'a S) -> &'a S::Value {
        assert_eq!(self.id, ctx.store_id(), "object used with the wrong context");
        self.internal.get(ctx)
    }

    /// Returns a mutable reference to the object that this handle points to.
    pub fn get_mut<'a, S: ObjectStore<T>>(&self, ctx: &'a mut S) -> &'a mut S::Value {
        assert_eq!(self.id, ctx.store_id(), "object used with the wrong context");
        self.internal.get_mut(ctx)
    }

    /// Returns the internal handle contains within this handle.
    pub fn internal_handle(&self) -> InternalStoreHandle<T> {
        self.internal
    }

    /// Returns the ID of the context associated with the handle.
    pub fn store_id(&self) -> StoreId {
        self.id
    }

    /// Overrides the store id with a new ID
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
    _phantom: std::marker::PhantomData<fn() -> T>,
}

unsafe impl<T> Send for InternalStoreHandle<T> {}
unsafe impl<T> Sync for InternalStoreHandle<T> {}

#[cfg(feature = "artifact-size")]
impl<T> loupe::MemoryUsage for InternalStoreHandle<T> {
    fn size_of_val(&self, _tracker: &mut dyn loupe::MemoryUsageTracker) -> usize {
        std::mem::size_of_val(&self)
    }
}

impl<T> Clone for InternalStoreHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for InternalStoreHandle<T> {}

impl<T> fmt::Debug for InternalStoreHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

impl<T> InternalStoreHandle<T> {
    /// Moves the given object into an object store and returns a
    /// handle to it.
    pub fn new<S: ObjectStore<T> + ?Sized>(store: &mut S, value: S::Value) -> Self {
        let list = store.list_mut();
        let idx = NonZeroUsize::new(list.len() + 1).unwrap();
        list.push(value);
        Self {
            idx,
            _phantom: Default::default(),
        }
    }

    /// TODO document
    pub fn get<'a, S: ObjectStore<T>>(&self, ctx: &'a S) -> &'a S::Value {
        &ctx.list()[self.idx.get() - 1]
    }

    /// Returns a mutable reference to the object that this handle points to.
    pub fn get_mut<'a, S: ObjectStore<T>>(&self, ctx: &'a mut S) -> &'a mut S::Value {
        &mut ctx.list_mut()[self.idx.get() - 1]
    }

    /// TODO document
    pub fn index(&self) -> usize {
        self.idx.get()
    }

    /// TODO document
    pub fn from_index(idx: usize) -> Option<Self> {
        NonZeroUsize::new(idx).map(|idx| Self {
            idx,
            _phantom: Default::default(),
        })
    }
}
