use wasmer_vm::StoreObjects;

use crate::Store;

/// We require the context to have a fixed memory address for its lifetime since
/// various bits of the VM have raw pointers that point back to it. Hence we
/// wrap the actual context in a box.
pub(crate) struct ContextInner<T> {
    pub(crate) objects: StoreObjects,
    pub(crate) store: Store,
    pub(crate) data: T,
}

/// A context containing a set of WebAssembly instances, along with host state.
///
/// All WebAssembly instances must exist within a context. In the majority of
/// cases each instance will have its own context, but it is possible to have
/// multiple instances in a context when these instances need to interact with
/// each other, for example sharing a memory between instances or calling
/// functions in another instance.
///
/// The lifetimes of run-time WebAssembly objects, notably [`Instance`],
/// [`Memory`], [`Global`], [`Table`] and [`Function`] is tied to a context:
/// the backing memory for these objects is only freed when the context is
/// freed.
///
/// The `T` generic parameter allows arbitrary data to be attached to a context.
/// This data can be accessed using the [`Context::data`] and
/// [`Context::data_mut`] methods. Host functions defined using
/// [`Function::new`] and [`Function::new_native`] receive
/// a reference to the context when they are called.
pub struct Context<T> {
    pub(crate) inner: Box<ContextInner<T>>,
}

impl<T> Context<T> {
    /// Creates a new context with the given host state.
    // TODO: Eliminate the Store type and move its functionality into Engine.
    pub fn new(store: &Store, data: T) -> Self {
        Self {
            inner: Box::new(ContextInner {
                objects: Default::default(),
                store: store.clone(),
                data,
            }),
        }
    }

    /// Returns a reference to the host state in this context.
    pub fn data(&self) -> &T {
        &self.inner.data
    }

    /// Returns a mutable- reference to the host state in this context.
    pub fn data_mut(&mut self) -> &mut T {
        &mut self.inner.data
    }

    /// Drops the context and returns the host state that was stored in it.
    pub fn into_data(self) -> T {
        self.inner.data
    }

    /// Returns a reference to the `Store` of this context.
    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    /// For use with the C API
    /// # Safety
    ///
    /// This is unsafe.
    pub unsafe fn transmute_data<U>(&mut self) -> &mut Context<U> {
        core::mem::transmute::<&mut Self, &mut Context<U>>(self)
    }
}

/// A temporary handle to a [`Context`].
pub struct StoreRef<'a, T: 'a> {
    inner: &'a ContextInner<T>,
}

impl<'a, T> StoreRef<'a, T> {
    /// Returns a reference to the host state in this context.
    pub fn data(&self) -> &'a T {
        &self.inner.data
    }

    /// Returns a reference to the `Store` of this context.
    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    pub(crate) fn objects(&self) -> &'a StoreObjects {
        &self.inner.objects
    }
}

/// A temporary handle to a [`Context`].
pub struct FunctionEnv<'a, T: 'a> {
    inner: &'a mut ContextInner<T>,
}

impl<T> FunctionEnv<'_, T> {
    /// Returns a reference to the host state in this context.
    pub fn data(&self) -> &T {
        &self.inner.data
    }

    /// Returns a mutable- reference to the host state in this context.
    pub fn data_mut(&mut self) -> &mut T {
        &mut self.inner.data
    }

    pub(crate) fn objects_mut(&mut self) -> &mut StoreObjects {
        &mut self.inner.objects
    }

    /// Returns a reference to the `Store` of this context.
    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    pub(crate) fn as_raw(&self) -> *mut ContextInner<T> {
        self.inner as *const ContextInner<T> as *mut ContextInner<T>
    }

    pub(crate) unsafe fn from_raw(raw: *mut ContextInner<T>) -> Self {
        Self { inner: &mut *raw }
    }
}

/// Helper trait for a value that is convertible to a [`StoreRef`].
pub trait AsStoreRef {
    /// Host state associated with the [`Context`].
    type Data;

    /// Returns a `StoreRef` pointing to the underlying context.
    fn as_store_ref(&self) -> StoreRef<'_, Self::Data>;
}

/// Helper trait for a value that is convertible to a [`StoreMut`].
pub trait AsStoreMut: AsStoreRef {
    /// Returns a `StoreMut` pointing to the underlying context.
    fn as_store_mut(&mut self) -> FunctionEnv<'_, Self::Data>;
}

impl<T> AsStoreRef for Context<T> {
    type Data = T;

    fn as_store_ref(&self) -> StoreRef<'_, Self::Data> {
        StoreRef { inner: &self.inner }
    }
}
impl<T> AsStoreMut for Context<T> {
    fn as_store_mut(&mut self) -> FunctionEnv<'_, Self::Data> {
        FunctionEnv {
            inner: &mut self.inner,
        }
    }
}
impl<T> AsStoreRef for StoreRef<'_, T> {
    type Data = T;

    fn as_store_ref(&self) -> StoreRef<'_, Self::Data> {
        StoreRef { inner: self.inner }
    }
}
impl<T> AsStoreRef for FunctionEnv<'_, T> {
    type Data = T;

    fn as_store_ref(&self) -> StoreRef<'_, Self::Data> {
        StoreRef { inner: self.inner }
    }
}
impl<T> AsStoreMut for FunctionEnv<'_, T> {
    fn as_store_mut(&mut self) -> FunctionEnv<'_, Self::Data> {
        FunctionEnv { inner: self.inner }
    }
}
impl<T: AsStoreRef> AsStoreRef for &'_ T {
    type Data = T::Data;

    fn as_store_ref(&self) -> StoreRef<'_, Self::Data> {
        T::as_store_ref(*self)
    }
}
impl<T: AsStoreRef> AsStoreRef for &'_ mut T {
    type Data = T::Data;

    fn as_store_ref(&self) -> StoreRef<'_, Self::Data> {
        T::as_store_ref(*self)
    }
}
impl<T: AsStoreMut> AsStoreMut for &'_ mut T {
    fn as_store_mut(&mut self) -> FunctionEnv<'_, Self::Data> {
        T::as_store_mut(*self)
    }
}
