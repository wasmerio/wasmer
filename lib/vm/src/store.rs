use crate::{
    VMExceptionObj, VMExternObj, VMFunction, VMFunctionEnvironment, VMGlobal, VMInstance, VMMemory,
    VMTable, VMTag,
};
use core::slice::Iter;
use std::{cell::UnsafeCell, fmt, marker::PhantomData, num::NonZeroUsize, ptr::NonNull};
use wasmer_types::StoreId;

/// Trait to represent an object managed by a context. This is implemented on
/// the VM types managed by the context.
pub trait StoreObject<Object = Box<dyn std::any::Any + Send>>: Sized {
    /// List the objects in the store.
    fn list(ctx: &StoreObjects<Object>) -> &Vec<Self>;

    /// List the objects in the store, mutably.
    fn list_mut(ctx: &mut StoreObjects<Object>) -> &mut Vec<Self>;
}
macro_rules! impl_context_object {
    ($($field:ident => $ty:ty,)*) => {
        $(
            impl<Object> StoreObject<Object> for $ty {
                fn list(ctx: &StoreObjects<Object>) -> &Vec<Self> {
                    &ctx.$field
                }
                fn list_mut(ctx: &mut StoreObjects<Object>) -> &mut Vec<Self> {
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
    instances => VMInstance,
    memories => VMMemory,
    extern_objs => VMExternObj,
    exceptions => VMExceptionObj,
    tags => VMTag,
    function_environments => VMFunctionEnvironment<Object>,
}

/// Set of objects managed by a context.
pub struct StoreObjects<Object = Box<dyn std::any::Any + Send>> {
    id: StoreId,
    memories: Vec<VMMemory>,
    tables: Vec<VMTable>,
    globals: Vec<VMGlobal>,
    functions: Vec<VMFunction>,
    instances: Vec<VMInstance>,
    extern_objs: Vec<VMExternObj>,
    exceptions: Vec<VMExceptionObj>,
    tags: Vec<VMTag>,
    function_environments: Vec<VMFunctionEnvironment<Object>>,
}

impl<Object> std::fmt::Debug for StoreObjects<Object> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreObjects")
            .field("id", &self.id)
            .field("memories", &self.memories)
            .field("tables", &self.tables)
            .field("globals", &self.globals)
            .field("functions", &self.functions)
            .field("instances", &self.instances)
            .field("extern_objs", &self.extern_objs)
            .field("exceptions", &self.exceptions)
            .field("tags", &self.tags)
            .field("function_environments", &self.function_environments)
            .finish()
    }
}

impl<Object> Default for StoreObjects<Object> {
    fn default() -> Self {
        Self {
            id: Default::default(),
            memories: Default::default(),
            tables: Default::default(),
            globals: Default::default(),
            functions: Default::default(),
            instances: Default::default(),
            extern_objs: Default::default(),
            exceptions: Default::default(),
            tags: Default::default(),
            function_environments: Default::default(),
        }
    }
}

impl<Object> StoreObjects<Object> {
    /// Create a new instance of [`Self`]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: StoreId,
        memories: Vec<VMMemory>,
        tables: Vec<VMTable>,
        globals: Vec<VMGlobal>,
        functions: Vec<VMFunction>,
        instances: Vec<VMInstance>,
        extern_objs: Vec<VMExternObj>,
        exceptions: Vec<VMExceptionObj>,
        tags: Vec<VMTag>,
        function_environments: Vec<VMFunctionEnvironment<Object>>,
    ) -> Self {
        Self {
            id,
            memories,
            tables,
            globals,
            functions,
            instances,
            extern_objs,
            function_environments,
            exceptions,
            tags,
        }
    }

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
    pub fn get_2_mut<T: StoreObject<Object>>(
        &mut self,
        a: InternalStoreHandle<T, Object>,
        b: InternalStoreHandle<T, Object>,
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

    /// Return an immutable iterator over all globals
    pub fn iter_globals(&self) -> Iter<'_, VMGlobal> {
        self.globals.iter()
    }

    /// Return an vector of all globals and converted to u128
    pub fn as_u128_globals(&self) -> Vec<u128> {
        self.iter_globals()
            .map(|v| unsafe { v.vmglobal().as_ref().val.u128 })
            .collect()
    }

    /// Set a global, at index idx. Will panic if idx is out of range
    /// Safety: the caller should check taht the raw value is compatible
    /// with destination VMGlobal type
    pub fn set_global_unchecked(&self, idx: usize, val: u128) {
        assert!(idx < self.globals.len());
        unsafe {
            self.globals[idx].vmglobal().as_mut().val.u128 = val;
        }
    }
}

/// Handle to an object managed by a context.
///
/// Internally this is just an integer index into a context. A reference to the
/// context must be passed in separately to access the actual object.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct StoreHandle<T, Object = Box<dyn std::any::Any + Send>> {
    id: StoreId,
    internal: InternalStoreHandle<T, Object>,
}

impl<T, Object> Clone for StoreHandle<T, Object> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            internal: self.internal,
        }
    }
}

impl<T, Object> std::hash::Hash for StoreHandle<T, Object> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.internal.idx.hash(state);
    }
}

impl<Object, T: StoreObject<Object>> fmt::Debug for StoreHandle<T, Object> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoreHandle")
            .field("id", &self.id)
            .field("internal", &self.internal.index())
            .finish()
    }
}

impl<Object, T: StoreObject<Object>> PartialEq for StoreHandle<T, Object> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.internal == other.internal
    }
}

impl<Object, T: StoreObject<Object>> Eq for StoreHandle<T, Object> {}

impl<Object, T: StoreObject<Object>> StoreHandle<T, Object> {
    /// Moves the given object into a context and returns a handle to it.
    pub fn new(ctx: &mut StoreObjects<Object>, val: T) -> Self {
        Self {
            id: ctx.id,
            internal: InternalStoreHandle::new(ctx, val),
        }
    }

    /// Returns a reference to the object that this handle points to.
    pub fn get<'a>(&self, ctx: &'a StoreObjects<Object>) -> &'a T {
        assert_eq!(self.id, ctx.id, "object used with the wrong context");
        self.internal.get(ctx)
    }

    /// Returns a mutable reference to the object that this handle points to.
    pub fn get_mut<'a>(&self, ctx: &'a mut StoreObjects<Object>) -> &'a mut T {
        assert_eq!(self.id, ctx.id, "object used with the wrong context");
        self.internal.get_mut(ctx)
    }

    /// Returns the internal handle contains within this handle.
    pub fn internal_handle(&self) -> InternalStoreHandle<T, Object> {
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
    pub unsafe fn from_internal(id: StoreId, internal: InternalStoreHandle<T, Object>) -> Self {
        Self { id, internal }
    }
}

/// Internal handle to an object owned by the current context.
///
/// Unlike `StoreHandle` this does not track the context ID: it is only
/// intended to be used within objects already owned by a context.
#[repr(transparent)]
pub struct InternalStoreHandle<T, Object = Box<dyn std::any::Any + Send>> {
    // Use a NonZero here to reduce the size of Option<InternalStoreHandle>.
    idx: NonZeroUsize,
    marker: PhantomData<fn(Object) -> (T, Object)>,
}

#[cfg(feature = "artifact-size")]
impl<T, Object> loupe::MemoryUsage for InternalStoreHandle<T, Object> {
    fn size_of_val(&self, _tracker: &mut dyn loupe::MemoryUsageTracker) -> usize {
        std::mem::size_of_val(&self)
    }
}

impl<T, Object> Clone for InternalStoreHandle<T, Object> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T, Object> Copy for InternalStoreHandle<T, Object> {}

impl<T, Object> fmt::Debug for InternalStoreHandle<T, Object> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InternalStoreHandle")
            .field("idx", &self.idx)
            .finish()
    }
}
impl<T, Object> PartialEq for InternalStoreHandle<T, Object> {
    fn eq(&self, other: &Self) -> bool {
        self.idx == other.idx
    }
}
impl<T, Object> Eq for InternalStoreHandle<T, Object> {}

impl<Object, T: StoreObject<Object>> InternalStoreHandle<T, Object> {
    /// Moves the given object into a context and returns a handle to it.
    pub fn new(ctx: &mut StoreObjects<Object>, val: T) -> Self {
        let list = T::list_mut(ctx);
        let idx = NonZeroUsize::new(list.len() + 1).unwrap();
        list.push(val);
        Self {
            idx,
            marker: PhantomData,
        }
    }

    /// Returns a reference to the object that this handle points to.
    pub fn get<'a>(&self, ctx: &'a StoreObjects<Object>) -> &'a T {
        &T::list(ctx)[self.idx.get() - 1]
    }

    /// Returns a mutable reference to the object that this handle points to.
    pub fn get_mut<'a>(&self, ctx: &'a mut StoreObjects<Object>) -> &'a mut T {
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
            Self::Host(p) => unsafe { NonNull::new_unchecked(p.get()) },
            Self::Instance(p) => *p,
        }
    }
}

impl<T> std::fmt::Debug for MaybeInstanceOwned<T>
where
    T: std::fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(p) => {
                write!(f, "host(")?;
                p.as_ref().fmt(f)?;
                write!(f, ")")
            }
            Self::Instance(p) => {
                write!(f, "instance(")?;
                unsafe { p.as_ref().fmt(f)? };
                write!(f, ")")
            }
        }
    }
}
