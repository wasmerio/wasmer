use std::fmt;

/// We require the context to have a fixed memory address for its lifetime since
/// various bits of the VM have raw pointers that point back to it. Hence we
/// wrap the actual context in a box.
pub(crate) struct StoreInner {
    pub(crate) objects: StoreObjects,
}

/// The store represents all global state that can be manipulated by
/// WebAssembly programs. It consists of the runtime representation
/// of all instances of functions, tables, memories, and globals that
/// have been allocated during the lifetime of the abstract machine.
///
/// The `Store` holds the engine (that is —amongst many things— used to compile
/// the Wasm bytes into a valid module artifact), in addition to the
/// [`Tunables`] (that are used to create the memories, tables and globals).
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#store>
pub struct Store {
    pub(crate) inner: Box<StoreInner>,
}

impl Store {
    /// Creates a new `Store`.
    pub fn new() -> Self {
        Self {
            inner: Box::new(StoreInner {
                objects: Default::default(),
            }),
        }
    }

    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine. The
    /// tunables are excluded from the logic.
    pub fn same(_a: &Self, _b: &Self) -> bool {
        true
    }
}

impl PartialEq for Store {
    fn eq(&self, other: &Self) -> bool {
        Self::same(self, other)
    }
}

// This is required to be able to set the trap_handler in the
// Store.
unsafe impl Send for Store {}
unsafe impl Sync for Store {}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Store").finish()
    }
}

/// A trait represinting any object that lives in the `Store`.
pub trait StoreObject {
    /// Return true if the object `Store` is the same as the provided `Store`.
    fn comes_from_same_store(&self, _store: &Store) -> bool {
        true
    }
}

impl AsStoreRef for Store {
    fn as_store_ref(&self) -> StoreRef<'_> {
        StoreRef { inner: &self.inner }
    }
}
impl AsStoreMut for Store {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        StoreMut {
            inner: &mut self.inner,
        }
    }
    fn objects_mut(&mut self) -> &mut StoreObjects {
        &mut self.inner.objects
    }
}

/// A temporary handle to a [`Context`].
pub struct StoreRef<'a> {
    pub(crate) inner: &'a StoreInner,
}

impl<'a> StoreRef<'a> {
    pub(crate) fn objects(&self) -> &'a StoreObjects {
        &self.inner.objects
    }

    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine. The
    /// tunables are excluded from the logic.
    pub fn same(a: &Self, b: &Self) -> bool {
        a.inner.objects.id() == b.inner.objects.id()
    }
}

/// A temporary handle to a [`Context`].
pub struct StoreMut<'a> {
    pub(crate) inner: &'a mut StoreInner,
}

impl<'a> StoreMut<'a> {
    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine. The
    /// tunables are excluded from the logic.
    pub fn same(a: &Self, b: &Self) -> bool {
        a.inner.objects.id() == b.inner.objects.id()
    }

    pub(crate) fn as_raw(&self) -> *mut StoreInner {
        self.inner as *const StoreInner as *mut StoreInner
    }

    pub(crate) unsafe fn from_raw(raw: *mut StoreInner) -> Self {
        Self { inner: &mut *raw }
    }
}

/// Helper trait for a value that is convertible to a [`StoreRef`].
pub trait AsStoreRef {
    /// Returns a `StoreRef` pointing to the underlying context.
    fn as_store_ref(&self) -> StoreRef<'_>;
}

/// Helper trait for a value that is convertible to a [`StoreMut`].
pub trait AsStoreMut: AsStoreRef {
    /// Returns a `StoreMut` pointing to the underlying context.
    fn as_store_mut(&mut self) -> StoreMut<'_>;

    /// Returns the ObjectMutable
    fn objects_mut(&mut self) -> &mut StoreObjects;
}

impl AsStoreRef for StoreRef<'_> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        StoreRef { inner: self.inner }
    }
}

impl AsStoreRef for StoreMut<'_> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        StoreRef { inner: self.inner }
    }
}
impl AsStoreMut for StoreMut<'_> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        StoreMut { inner: self.inner }
    }
    fn objects_mut(&mut self) -> &mut StoreObjects {
        &mut self.inner.objects
    }
}

impl<T: AsStoreRef> AsStoreRef for &'_ T {
    fn as_store_ref(&self) -> StoreRef<'_> {
        T::as_store_ref(*self)
    }
}
impl<T: AsStoreRef> AsStoreRef for &'_ mut T {
    fn as_store_ref(&self) -> StoreRef<'_> {
        T::as_store_ref(*self)
    }
}
impl<T: AsStoreMut> AsStoreMut for &'_ mut T {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        T::as_store_mut(*self)
    }
    fn objects_mut(&mut self) -> &mut StoreObjects {
        T::objects_mut(*self)
    }
}

pub use objects::*;

mod objects {
    use crate::js::{
        export::{VMFunction, VMGlobal, VMMemory, VMTable},
        function_env::VMFunctionEnvironment,
    };
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
    #[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
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
        fn list(store: &StoreObjects) -> &Vec<Self>;
        fn list_mut(store: &mut StoreObjects) -> &mut Vec<Self>;
    }

    macro_rules! impl_store_object {
    ($($field:ident => $ty:ty,)*) => {
        $(
            impl StoreObject for $ty {
                fn list(store: &StoreObjects) -> &Vec<Self> {
                    &store.$field
                }
                fn list_mut(store: &mut StoreObjects) -> &mut Vec<Self> {
                    &mut store.$field
                }
            }
        )*
    };
}

    impl_store_object! {
        functions => VMFunction,
        tables => VMTable,
        globals => VMGlobal,
        memories => VMMemory,
        instances => js_sys::WebAssembly::Instance,
        function_environments => VMFunctionEnvironment,
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
        function_environments: Vec<VMFunctionEnvironment>,
    }

    impl StoreObjects {
        /// Returns the ID of this context.
        pub fn id(&self) -> StoreId {
            self.id
        }

        /// Sets the ID of this store
        pub fn set_id(&mut self, id: StoreId) {
            self.id = id;
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
        pub fn new(store: &mut StoreObjects, val: T) -> Self {
            Self {
                id: store.id,
                internal: InternalStoreHandle::new(store, val),
            }
        }

        /// Returns a reference to the object that this handle points to.
        pub fn get<'a>(&self, store: &'a StoreObjects) -> &'a T {
            assert_eq!(self.id, store.id, "object used with the wrong context");
            self.internal.get(store)
        }

        /// Returns a mutable reference to the object that this handle points to.
        pub fn get_mut<'a>(&self, store: &'a mut StoreObjects) -> &'a mut T {
            assert_eq!(self.id, store.id, "object used with the wrong context");
            self.internal.get_mut(store)
        }

        /// Returns the internal handle contains within this handle.
        pub fn internal_handle(&self) -> InternalStoreHandle<T> {
            self.internal
        }

        /// Returns the ID of the context associated with the handle.
        pub fn store_id(&self) -> StoreId {
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

    /// Data used by the generated code is generally located inline within the
    /// `VMContext` for items defined in an instance. Host-defined objects are
    /// allocated separately and owned directly by the context.
    #[allow(dead_code)]
    pub enum MaybeInstanceOwned<T> {
        /// The data is owned here.
        Host(Box<UnsafeCell<T>>),

        /// The data is stored inline in the `VMContext` of an instance.
        Instance(NonNull<T>),
    }

    impl<T> MaybeInstanceOwned<T> {
        /// Returns underlying pointer to the VM data.
        #[allow(dead_code)]
        pub fn as_ptr(&self) -> NonNull<T> {
            match self {
                MaybeInstanceOwned::Host(p) => unsafe { NonNull::new_unchecked(p.get()) },
                MaybeInstanceOwned::Instance(p) => *p,
            }
        }
    }
}
