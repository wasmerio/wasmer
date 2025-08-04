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


/// Trait to represent an object managed by a context. This is implemented on
/// the VM types managed by the context.
pub trait ObjectStore<K> {
    /// The type of data this type refers to in the store.
    type Value;

    /// Get the unique ID of the store.
    fn store_id(&self) -> crate::StoreId;

    /// List the objects in the store.
    fn list(&self) -> &Vec<Self::Value>;

    /// List the objects in the store, mutably.
    fn list_mut(&mut self) -> &mut Vec<Self::Value>;
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
