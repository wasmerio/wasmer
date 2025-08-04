use crate::{
    VMExceptionObj, VMExternObj, VMFunction, VMFunctionEnvironment, VMGlobal, VMInstance, VMMemory,
    VMTable, VMTag,
};
use core::slice::Iter;
use std::{cell::UnsafeCell, fmt, marker::PhantomData, num::NonZeroUsize, ptr::NonNull};
use wasmer_types::{BoxStoreObject, ObjectStore, StoreId};

macro_rules! impl_context_object {
    ($Self:ident $(<$($Self_params:ident),*>)? . $field:ident : $Value:ident$(<$($Value_params:ident),*>)?) => {
        impl $(<$($Self_params),*>)? ObjectStore<$Value> for $Self $(<$($Self_params),*>)? {
            type Value = $Value$(<$($Value_params),*>)?;

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
}

impl_context_object!(StoreObjects<Object>.functions: VMFunction);
impl_context_object!(StoreObjects<Object>.tables: VMTable);
impl_context_object!(StoreObjects<Object>.globals: VMGlobal);
impl_context_object!(StoreObjects<Object>.instances: VMInstance<Object>);
impl_context_object!(StoreObjects<Object>.memories: VMMemory);
impl_context_object!(StoreObjects<Object>.extern_objs: VMExternObj);
impl_context_object!(StoreObjects<Object>.exceptions: VMExceptionObj);
impl_context_object!(StoreObjects<Object>.function_environments: VMFunctionEnvironment<Object>);
impl_context_object!(StoreObjects<Object>.tags: VMTag);

/// Set of objects managed by a context.
pub struct StoreObjects<Object = BoxStoreObject> {
    id: StoreId,
    memories: Vec<VMMemory>,
    tables: Vec<VMTable>,
    globals: Vec<VMGlobal>,
    functions: Vec<VMFunction>,
    instances: Vec<VMInstance<Object>>,
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
        instances: Vec<VMInstance<Object>>,
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
    pub fn get_2_mut<T>(
        &mut self,
        a: InternalStoreHandle<T>,
        b: InternalStoreHandle<T>,
    ) -> (&mut <Self as ObjectStore<T>>::Value, &mut <Self as ObjectStore<T>>::Value)
    where
        Self: ObjectStore<T>,
    {
        assert_ne!(a.index(), b.index());
        let list = self.list_mut();
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
pub struct StoreHandle<T> {
    id: StoreId,
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
    /// Moves the given object into a context and returns a handle to it.
    pub fn new<Store: ObjectStore<T>>(ctx: &mut Store, val: Store::Value) -> Self {
        Self {
            id: ctx.store_id(),
            internal: InternalStoreHandle::new(ctx, val),
        }
    }

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
    marker: PhantomData<fn() -> T>,
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
    /// Moves the given object into a context and returns a handle to it.
    pub fn new<S: ObjectStore<T>>(ctx: &mut S, val: S::Value) -> Self {
        let list = ctx.list_mut();
        let idx = NonZeroUsize::new(list.len() + 1).unwrap();
        list.push(val);
        Self {
            idx,
            marker: PhantomData,
        }
    }

    /// Returns a reference to the object that this handle points to.
    pub fn get<'a, S: ObjectStore<T>>(&self, ctx: &'a S) -> &'a S::Value {
        &ctx.list()[self.idx.get() - 1]
    }

    /// Returns a mutable reference to the object that this handle points to.
    pub fn get_mut<'a, S: ObjectStore<T>>(&self, ctx: &'a mut S) -> &'a mut S::Value {
        &mut ctx.list_mut()[self.idx.get() - 1]
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

impl InternalStoreHandle<VMMemory> {
    /// Moves the given memory object into a list of memories and
    /// returns a handle to it.
    // TODO maybe come up with a system of references to the different
    // object lists in the store
    pub fn new_memory(memories: &mut Vec<VMMemory>, val: VMMemory) -> Self {
        let idx = NonZeroUsize::new(memories.len() + 1).unwrap();
        memories.push(val);
        Self {
            idx,
            marker: PhantomData,
        }
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
