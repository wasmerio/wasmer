use wasmer_vm::StoreObjects;

/// The trait that every concrete store handle must implement.
pub trait StoreHandleLike<T>: std::fmt::Debug {
    /// Get a reference to the object stored in this handle.
    fn get<'a>(&self, ctx: &'a StoreObjects) -> &'a T;

    /// Get a mutable reference to the object stored in this handle.
    fn get_mut<'a>(&self, ctx: &'a StoreObjects) -> &'a mut T;

    /// Create a boxed clone of this implementer.
    fn clone_boxed(&self) -> Box<dyn StoreHandleLike<T>>;

    /// Cast to [`std::any::Any`].
    ///
    /// # Note
    /// This function is here just because one can't impose [`PartialEq`] as a supertrait,
    /// see [`StoreHandleLike::cmp`].
    fn as_any(&self) -> &dyn std::any::Any;

    /// Compare this store handle to another.
    ///
    /// # Note
    /// This function is here just because one can't impose [`PartialEq`] as a supertrait.
    fn cmp(&self, other: &dyn StoreHandleLike<T>) -> std::cmp::Ordering;

    /// Get a unique hash for the object stored in this handle.
    ///
    /// # Note
    /// This function is here just because one can't impose [`std::hash::Hash`] as a supertrait.
    fn hash(&self) -> u64;
}

/// A newtype for references to those that implement [<VM $name Like>].
pub type StoreHandle<T> = Box<dyn StoreHandleLike<T>>;

/// The trait implemented by all those that can create new store handles.
pub trait StoreHandleCreator {
    /// Create a new [`StoreHandle`] from a value of the given type `T`
    fn store_handle_from_value<T>(&mut self, val: T) -> StoreHandle<T>
    where
        Self: Sized;
}
