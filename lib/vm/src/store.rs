use crate::{
    VMExceptionObj, VMExternObj, VMFunction, VMFunctionEnvironment, VMGlobal, VMInstance, VMMemory,
    VMTable, VMTag,
};
use core::slice::Iter;
use std::{cell::UnsafeCell, fmt, ptr::NonNull};
use wasmer_types::{BoxStoreObject, ObjectStoreOf, StoreId, impl_object_store};

pub use wasmer_types::{InternalStoreHandle, StoreHandle};

impl_object_store!(StoreObjects<Object> {
    functions: VMFunction,
    tables: VMTable,
    globals: VMGlobal,
    instances: VMInstance<Object>,
    memories: VMMemory,
    extern_objs: VMExternObj,
    exceptions: VMExceptionObj,
    function_environments: VMFunctionEnvironment<Object>,
    tags: VMTag,
});

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
    ) -> (&mut <Self as ObjectStoreOf<T>>::Value, &mut <Self as ObjectStoreOf<T>>::Value)
    where
        Self: ObjectStoreOf<T>,
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
