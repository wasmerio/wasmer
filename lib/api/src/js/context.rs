#![allow(dead_code)]
use crate::Store;

/// We require the context to have a fixed memory address for its lifetime since
/// various bits of the VM have raw pointers that point back to it. Hence we
/// wrap the actual context in a box.
pub(crate) struct ContextInner<T> {
    pub(crate) objects: ContextObjects,
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
    pub fn new(data: T) -> Self {
        Self {
            inner: Box::new(ContextInner {
                objects: Default::default(),
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
}

/// A temporary handle to a [`Context`].
pub struct ContextRef<'a, T: 'a> {
    inner: &'a ContextInner<T>,
}

impl<'a, T> ContextRef<'a, T> {
    /// Returns a reference to the host state in this context.
    pub fn data(&self) -> &'a T {
        &self.inner.data
    }

    /// Returns a reference to the `Store` of this context.
    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    pub(crate) fn objects(&self) -> &'a ContextObjects {
        &self.inner.objects
    }
}

/// A temporary handle to a [`Context`].
pub struct ContextMut<'a, T: 'a> {
    inner: &'a mut ContextInner<T>,
}

impl<T> ContextMut<'_, T> {
    /// Returns a reference to the host state in this context.
    pub fn data(&self) -> &T {
        &self.inner.data
    }

    /// Returns a mutable- reference to the host state in this context.
    pub fn data_mut(&mut self) -> &mut T {
        &mut self.inner.data
    }

    pub(crate) fn objects_mut(&mut self) -> &mut ContextObjects {
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

/// Helper trait for a value that is convertible to a [`ContextRef`].
pub trait AsContextRef {
    /// Host state associated with the [`Context`].
    type Data;

    /// Returns a `ContextRef` pointing to the underlying context.
    fn as_context_ref(&self) -> ContextRef<'_, Self::Data>;
}

/// Helper trait for a value that is convertible to a [`ContextMut`].
pub trait AsContextMut: AsContextRef {
    /// Returns a `ContextMut` pointing to the underlying context.
    fn as_context_mut(&mut self) -> ContextMut<'_, Self::Data>;
}

impl<T> AsContextRef for Context<T> {
    type Data = T;

    fn as_context_ref(&self) -> ContextRef<'_, Self::Data> {
        ContextRef { inner: &self.inner }
    }
}
impl<T> AsContextMut for Context<T> {
    fn as_context_mut(&mut self) -> ContextMut<'_, Self::Data> {
        ContextMut {
            inner: &mut self.inner,
        }
    }
}
impl<T> AsContextRef for ContextRef<'_, T> {
    type Data = T;

    fn as_context_ref(&self) -> ContextRef<'_, Self::Data> {
        ContextRef { inner: self.inner }
    }
}
impl<T> AsContextRef for ContextMut<'_, T> {
    type Data = T;

    fn as_context_ref(&self) -> ContextRef<'_, Self::Data> {
        ContextRef { inner: self.inner }
    }
}
impl<T> AsContextMut for ContextMut<'_, T> {
    fn as_context_mut(&mut self) -> ContextMut<'_, Self::Data> {
        ContextMut { inner: self.inner }
    }
}
impl<T: AsContextRef> AsContextRef for &'_ T {
    type Data = T::Data;

    fn as_context_ref(&self) -> ContextRef<'_, Self::Data> {
        T::as_context_ref(*self)
    }
}
impl<T: AsContextRef> AsContextRef for &'_ mut T {
    type Data = T::Data;

    fn as_context_ref(&self) -> ContextRef<'_, Self::Data> {
        T::as_context_ref(*self)
    }
}
impl<T: AsContextMut> AsContextMut for &'_ mut T {
    fn as_context_mut(&mut self) -> ContextMut<'_, Self::Data> {
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
    pub struct ContextId(NonZeroU64);

    impl Default for ContextId {
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
    pub trait ContextObject: Sized {
        fn list(ctx: &ContextObjects) -> &Vec<Self>;
        fn list_mut(ctx: &mut ContextObjects) -> &mut Vec<Self>;
    }

    macro_rules! impl_context_object {
    ($($field:ident => $ty:ty,)*) => {
        $(
            impl ContextObject for $ty {
                fn list(ctx: &ContextObjects) -> &Vec<Self> {
                    &ctx.$field
                }
                fn list_mut(ctx: &mut ContextObjects) -> &mut Vec<Self> {
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
    pub struct ContextObjects {
        id: ContextId,
        memories: Vec<VMMemory>,
        tables: Vec<VMTable>,
        globals: Vec<VMGlobal>,
        functions: Vec<VMFunction>,
        instances: Vec<js_sys::WebAssembly::Instance>,
    }

    impl ContextObjects {
        /// Returns the ID of this context.
        pub fn id(&self) -> ContextId {
            self.id
        }

        /// Returns a pair of mutable references from two handles.
        ///
        /// Panics if both handles point to the same object.
        pub fn get_2_mut<T: ContextObject>(
            &mut self,
            a: InternalContextHandle<T>,
            b: InternalContextHandle<T>,
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
    pub struct ContextHandle<T> {
        id: ContextId,
        internal: InternalContextHandle<T>,
    }

    impl<T> core::cmp::PartialEq for ContextHandle<T> {
        fn eq(&self, other: &Self) -> bool {
            self.id == other.id
        }
    }
    impl<T> Clone for ContextHandle<T> {
        fn clone(&self) -> Self {
            Self {
                id: self.id,
                internal: self.internal,
            }
        }
    }

    impl<T: ContextObject> fmt::Debug for ContextHandle<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("ContextHandle")
                .field("id", &self.id)
                .field("internal", &self.internal.index())
                .finish()
        }
    }

    impl<T: ContextObject> ContextHandle<T> {
        /// Moves the given object into a context and returns a handle to it.
        pub fn new(ctx: &mut ContextObjects, val: T) -> Self {
            Self {
                id: ctx.id,
                internal: InternalContextHandle::new(ctx, val),
            }
        }

        /// Returns a reference to the object that this handle points to.
        pub fn get<'a>(&self, ctx: &'a ContextObjects) -> &'a T {
            assert_eq!(self.id, ctx.id, "object used with the wrong context");
            self.internal.get(ctx)
        }

        /// Returns a mutable reference to the object that this handle points to.
        pub fn get_mut<'a>(&self, ctx: &'a mut ContextObjects) -> &'a mut T {
            assert_eq!(self.id, ctx.id, "object used with the wrong context");
            self.internal.get_mut(ctx)
        }

        /// Returns the internal handle contains within this handle.
        pub fn internal_handle(&self) -> InternalContextHandle<T> {
            self.internal
        }

        /// Returns the ID of the context associated with the handle.
        pub fn store_id(&self) -> ContextId {
            self.id
        }

        /// Constructs a `ContextHandle` from a `ContextId` and an `InternalContextHandle`.
        ///
        /// # Safety
        /// Handling `InternalContextHandle` values is unsafe because they do not track context ID.
        pub unsafe fn from_internal(id: ContextId, internal: InternalContextHandle<T>) -> Self {
            Self { id, internal }
        }
    }

    /// Internal handle to an object owned by the current context.
    ///
    /// Unlike `ContextHandle` this does not track the context ID: it is only
    /// intended to be used within objects already owned by a context.
    #[repr(transparent)]
    pub struct InternalContextHandle<T> {
        // Use a NonZero here to reduce the size of Option<InternalContextHandle>.
        idx: NonZeroUsize,
        marker: PhantomData<fn() -> T>,
    }

    impl<T> Clone for InternalContextHandle<T> {
        fn clone(&self) -> Self {
            *self
        }
    }
    impl<T> Copy for InternalContextHandle<T> {}

    impl<T> fmt::Debug for InternalContextHandle<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("InternalContextHandle")
                .field("idx", &self.idx)
                .finish()
        }
    }
    impl<T> PartialEq for InternalContextHandle<T> {
        fn eq(&self, other: &Self) -> bool {
            self.idx == other.idx
        }
    }
    impl<T> Eq for InternalContextHandle<T> {}

    impl<T: ContextObject> InternalContextHandle<T> {
        /// Moves the given object into a context and returns a handle to it.
        pub fn new(ctx: &mut ContextObjects, val: T) -> Self {
            let list = T::list_mut(ctx);
            let idx = NonZeroUsize::new(list.len() + 1).unwrap();
            list.push(val);
            Self {
                idx,
                marker: PhantomData,
            }
        }

        /// Returns a reference to the object that this handle points to.
        pub fn get<'a>(&self, ctx: &'a ContextObjects) -> &'a T {
            &T::list(ctx)[self.idx.get() - 1]
        }

        /// Returns a mutable reference to the object that this handle points to.
        pub fn get_mut<'a>(&self, ctx: &'a mut ContextObjects) -> &'a mut T {
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
