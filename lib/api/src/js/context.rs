#![allow(dead_code)]
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

    /// Returns the raw pointer of the context
    pub(crate) fn as_raw(&self) -> *mut ContextInner<T> {
        self.inner as *const ContextInner<T> as *mut ContextInner<T>
    }

    /// Constructs the context from the raw pointer
    pub(crate) unsafe fn from_raw(raw: *mut ContextInner<T>) -> Self {
        Self { inner: &mut *raw }
    }
}

/// Helper trait for a value that is convertible to a [`StoreRef`].
pub trait AsStoreRef {
    /// Host state associated with the [`Context`].
    type Data;

    /// Returns a `StoreRef` pointing to the underlying context.
    fn as_context_ref(&self) -> StoreRef<'_, Self::Data>;
}

/// Helper trait for a value that is convertible to a [`StoreMut`].
pub trait AsStoreMut: AsStoreRef {
    /// Returns a `StoreMut` pointing to the underlying context.
    fn as_context_mut(&mut self) -> FunctionEnv<'_, Self::Data>;
}

impl<T> AsStoreRef for Context<T> {
    type Data = T;

    fn as_context_ref(&self) -> StoreRef<'_, Self::Data> {
        StoreRef { inner: &self.inner }
    }
}
impl<T> AsStoreMut for Context<T> {
    fn as_context_mut(&mut self) -> FunctionEnv<'_, Self::Data> {
        FunctionEnv {
            inner: &mut self.inner,
        }
    }
}
impl<T> AsStoreRef for StoreRef<'_, T> {
    type Data = T;

    fn as_context_ref(&self) -> StoreRef<'_, Self::Data> {
        StoreRef { inner: self.inner }
    }
}
impl<T> AsStoreRef for FunctionEnv<'_, T> {
    type Data = T;

    fn as_context_ref(&self) -> StoreRef<'_, Self::Data> {
        StoreRef { inner: self.inner }
    }
}
impl<T> AsStoreMut for FunctionEnv<'_, T> {
    fn as_context_mut(&mut self) -> FunctionEnv<'_, Self::Data> {
        FunctionEnv { inner: self.inner }
    }
}
impl<T: AsStoreRef> AsStoreRef for &'_ T {
    type Data = T::Data;

    fn as_context_ref(&self) -> StoreRef<'_, Self::Data> {
        T::as_context_ref(*self)
    }
}
impl<T: AsStoreRef> AsStoreRef for &'_ mut T {
    type Data = T::Data;

    fn as_context_ref(&self) -> StoreRef<'_, Self::Data> {
        T::as_context_ref(*self)
    }
}
impl<T: AsStoreMut> AsStoreMut for &'_ mut T {
    fn as_context_mut(&mut self) -> FunctionEnv<'_, Self::Data> {
        T::as_context_mut(*self)
    }
}

pub use objects::*;
mod objects {
    use crate::js::export::{VMFunction, VMGlobal, VMMemory, VMTable};
    use std::{
        cell::UnsafeCell,
        fmt,
        marker::PhantomData,
        num::{NonZeroU64, NonZeroUsize},
        ptr::NonNull,
        sync::atomic::{AtomicU64, Ordering},
    };

    /// Unique ID to identify a context.
    ///
    /// Every handle to an object managed by a context also contains the ID of the
    /// context. This is used to check that a handle is always used with the
    /// correct context.
    #[derive(Debug, Copy, Clone, Eq, PartialEq)]
    pub struct StoreId(NonZeroU64);

    impl Default for StoreId {
        // Allocates a unique ID for a new context.
        fn default() -> Self {
            // No overflow checking is needed here: overflowing this would take
            // thousands of years.
            static NEXT_ID: AtomicU64 = AtomicU64::new(1);
            Self(NonZeroU64::new(NEXT_ID.fetch_add(1, Ordering::Relaxed)).unwrap())
        }
    }

    /// Trait to represent an object managed by a context. This is implemented on
    /// the VM types managed by the context.
    pub trait StoreObject: Sized {
        fn list(ctx: &StoreObjects) -> &Vec<Self>;
        fn list_mut(ctx: &mut StoreObjects) -> &mut Vec<Self>;
    }

    macro_rules! impl_context_object {
    ($($field:ident => $ty:ty,)*) => {
        $(
            impl StoreObject for $ty {
                fn list(ctx: &StoreObjects) -> &Vec<Self> {
                    &ctx.$field
                }
                fn list_mut(ctx: &mut StoreObjects) -> &mut Vec<Self> {
                    &mut ctx.$field
                }
            }
        )*
    };
}

    impl_context_object! {
        functions => VMFunction,
        tables => VMTable,
        globals => VMGlobal,
        memories => VMMemory,
        instances => js_sys::WebAssembly::Instance,
    }

    /// Set of objects managed by a context.
    #[derive(Default)]
    pub struct StoreObjects {
        id: StoreId,
        memories: Vec<VMMemory>,
        tables: Vec<VMTable>,
        globals: Vec<VMGlobal>,
        functions: Vec<VMFunction>,
        instances: Vec<js_sys::WebAssembly::Instance>,
    }

    impl StoreObjects {
        /// Returns the ID of this context.
        pub fn id(&self) -> StoreId {
            self.id
        }

        /// Returns a pair of mutable references from two handles.
        ///
        /// Panics if both handles point to the same object.
        pub fn get_2_mut<T: StoreObject>(
            &mut self,
            a: InternalStoreHandle<T>,
            b: InternalStoreHandle<T>,
        ) -> (&mut T, &mut T) {
            assert_ne!(a.index(), b.index());
            let list = T::list_mut(self);
            if a.index() < b.index() {
                let (low, high) = list.split_at_mut(b.index());
                (&mut low[a.index()], &mut high[0])
            } else {
                let (low, high) = list.split_at_mut(a.index());
                (&mut high[0], &mut low[a.index()])
            }
        }
    }

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
    impl<T> Clone for StoreHandle<T> {
        fn clone(&self) -> Self {
            Self {
                id: self.id,
                internal: self.internal,
            }
        }
    }

    impl<T: StoreObject> fmt::Debug for StoreHandle<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("StoreHandle")
                .field("id", &self.id)
                .field("internal", &self.internal.index())
                .finish()
        }
    }

    impl<T: StoreObject> StoreHandle<T> {
        /// Moves the given object into a context and returns a handle to it.
        pub fn new(ctx: &mut StoreObjects, val: T) -> Self {
            Self {
                id: ctx.id,
                internal: InternalStoreHandle::new(ctx, val),
            }
        }

        /// Returns a reference to the object that this handle points to.
        pub fn get<'a>(&self, ctx: &'a StoreObjects) -> &'a T {
            assert_eq!(self.id, ctx.id, "object used with the wrong context");
            self.internal.get(ctx)
        }

        /// Returns a mutable reference to the object that this handle points to.
        pub fn get_mut<'a>(&self, ctx: &'a mut StoreObjects) -> &'a mut T {
            assert_eq!(self.id, ctx.id, "object used with the wrong context");
            self.internal.get_mut(ctx)
        }

        /// Returns the internal handle contains within this handle.
        pub fn internal_handle(&self) -> InternalStoreHandle<T> {
            self.internal
        }

        /// Returns the ID of the context associated with the handle.
        pub fn context_id(&self) -> StoreId {
            self.id
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

    impl<T: StoreObject> InternalStoreHandle<T> {
        /// Moves the given object into a context and returns a handle to it.
        pub fn new(ctx: &mut StoreObjects, val: T) -> Self {
            let list = T::list_mut(ctx);
            let idx = NonZeroUsize::new(list.len() + 1).unwrap();
            list.push(val);
            Self {
                idx,
                marker: PhantomData,
            }
        }

        /// Returns a reference to the object that this handle points to.
        pub fn get<'a>(&self, ctx: &'a StoreObjects) -> &'a T {
            &T::list(ctx)[self.idx.get() - 1]
        }

        /// Returns a mutable reference to the object that this handle points to.
        pub fn get_mut<'a>(&self, ctx: &'a mut StoreObjects) -> &'a mut T {
            &mut T::list_mut(ctx)[self.idx.get() - 1]
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

    /// Data used by the generated code is generally located inline within the
    /// `VMContext` for items defined in an instance. Host-defined objects are
    /// allocated separately and owned directly by the context.
    pub enum MaybeInstanceOwned<T> {
        /// The data is owned here.
        Host(Box<UnsafeCell<T>>),

        /// The data is stored inline in the `VMContext` of an instance.
        Instance(NonNull<T>),
    }

    impl<T> MaybeInstanceOwned<T> {
        /// Returns underlying pointer to the VM data.
        pub fn as_ptr(&self) -> NonNull<T> {
            match self {
                MaybeInstanceOwned::Host(p) => unsafe { NonNull::new_unchecked(p.get()) },
                MaybeInstanceOwned::Instance(p) => *p,
            }
        }
    }
}
