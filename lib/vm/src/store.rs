use crate::{
    VMExceptionObj, VMExternObj, VMFunction, VMFunctionEnvironment, VMGlobal, VMInstance, VMMemory,
    VMTable, VMTag,
};
use core::slice::Iter;
use fnv::FnvHashMap;
use std::{cell::UnsafeCell, fmt, marker::PhantomData, num::NonZeroUsize, ptr::NonNull};
use wasmer_types::StoreId;

/// Trait to represent a collection of objects that can live in a store.
pub trait StoreObjectList<T> {
    /// Gets the object at the given index.
    fn get(&self, index: NonZeroUsize) -> &T;

    /// Mutably gets the object at the given index.
    fn get_mut(&mut self, index: NonZeroUsize) -> &mut T;

    /// Adds an object to the list, returning its index.
    fn append(&mut self, obj: T) -> NonZeroUsize;

    /// Gets a pair of mutable references to two elements in the list.
    /// Only supported for the Vec<T> implementation.
    fn pair_mut(&mut self, a: NonZeroUsize, b: NonZeroUsize) -> Result<(&mut T, &mut T), ()>;

    /// Deletes the given index from the list, if the operation
    /// is supported by the specific implementation. Notably,
    /// the implementation for Vec<T> does *not* support deletion
    /// because deleting an element would mess up existing indices.
    #[allow(dead_code)]
    // FIXME: remove the #allow after we implement exception deallocation
    fn try_delete(&mut self, index: NonZeroUsize) -> Result<Option<T>, ()>;
}

impl<T> StoreObjectList<T> for Vec<T> {
    fn get(&self, index: NonZeroUsize) -> &T {
        &self[index.get() - 1]
    }

    fn get_mut(&mut self, index: NonZeroUsize) -> &mut T {
        &mut self[index.get() - 1]
    }

    fn append(&mut self, obj: T) -> NonZeroUsize {
        let idx = self.len();
        self.push(obj);
        NonZeroUsize::new(idx + 1).unwrap()
    }

    fn pair_mut(&mut self, a: NonZeroUsize, b: NonZeroUsize) -> Result<(&mut T, &mut T), ()> {
        if a == b {
            panic!("attempted to get two mutable references to the same object");
        }
        if a.get() < b.get() {
            let (low, high) = self.split_at_mut(b.get() - 1);
            Ok((&mut low[a.get() - 1], &mut high[0]))
        } else {
            let (low, high) = self.split_at_mut(a.get() - 1);
            Ok((&mut high[0], &mut low[b.get() - 1]))
        }
    }

    fn try_delete(&mut self, _index: NonZeroUsize) -> Result<Option<T>, ()> {
        Err(())
    }
}

/// A collection of exceptions managed by a store. Exceptions need to be
/// (de)allocated on the fly, so they can't use the same Vec<T> storage
/// as other store objects.
#[derive(Debug)]
pub struct ExceptionCollection {
    next_index: usize,
    exceptions: FnvHashMap<NonZeroUsize, VMExceptionObj>,
}

impl Default for ExceptionCollection {
    fn default() -> Self {
        Self::new()
    }
}

impl ExceptionCollection {
    fn new() -> Self {
        Self {
            next_index: 1,
            exceptions: FnvHashMap::default(),
        }
    }
}

impl StoreObjectList<VMExceptionObj> for ExceptionCollection {
    fn get(&self, index: NonZeroUsize) -> &VMExceptionObj {
        self.exceptions.get(&index).expect("invalid index")
    }

    fn get_mut(&mut self, index: NonZeroUsize) -> &mut VMExceptionObj {
        self.exceptions.get_mut(&index).expect("invalid index")
    }

    fn append(&mut self, obj: VMExceptionObj) -> NonZeroUsize {
        let idx = NonZeroUsize::new(self.next_index).unwrap();
        self.next_index += 1;
        self.exceptions.insert(idx, obj);
        idx
    }

    fn pair_mut(
        &mut self,
        _a: NonZeroUsize,
        _b: NonZeroUsize,
    ) -> Result<(&mut VMExceptionObj, &mut VMExceptionObj), ()> {
        Err(())
    }

    fn try_delete(&mut self, index: NonZeroUsize) -> Result<Option<VMExceptionObj>, ()> {
        Ok(self.exceptions.remove(&index))
    }
}

/// Trait to represent an object managed by a context. This is implemented on
/// the VM types managed by the context.
pub trait StoreObject: Sized {
    /// The type of the list that holds instances of this object.
    type List: StoreObjectList<Self>;

    /// List the objects in the store.
    fn list(ctx: &StoreObjects) -> &Self::List;

    /// List the objects in the store, mutably.
    fn list_mut(ctx: &mut StoreObjects) -> &mut Self::List;
}

macro_rules! impl_context_object {
    ($($field:ident => $ty:ty => $list:ty,)*) => {
        $(
            impl StoreObject for $ty {
                type List = $list;

                fn list(ctx: &StoreObjects) -> &Self::List {
                    &ctx.$field
                }
                fn list_mut(ctx: &mut StoreObjects) -> &mut Self::List {
                    &mut ctx.$field
                }
            }
        )*
    };
}

impl_context_object! {
    functions => VMFunction => Vec<VMFunction>,
    tables => VMTable => Vec<VMTable>,
    globals => VMGlobal => Vec<VMGlobal>,
    instances => VMInstance => Vec<VMInstance>,
    memories => VMMemory => Vec<VMMemory>,
    extern_objs => VMExternObj => Vec<VMExternObj>,
    exceptions => VMExceptionObj => ExceptionCollection,
    tags => VMTag => Vec<VMTag>,
    function_environments => VMFunctionEnvironment => Vec<VMFunctionEnvironment>,
}

/// Set of objects managed by a context.
#[derive(Debug, Default)]
pub struct StoreObjects {
    id: StoreId,
    memories: Vec<VMMemory>,
    tables: Vec<VMTable>,
    globals: Vec<VMGlobal>,
    functions: Vec<VMFunction>,
    instances: Vec<VMInstance>,
    extern_objs: Vec<VMExternObj>,
    exceptions: ExceptionCollection,
    tags: Vec<VMTag>,
    function_environments: Vec<VMFunctionEnvironment>,
}

impl StoreObjects {
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
        exceptions: ExceptionCollection,
        tags: Vec<VMTag>,
        function_environments: Vec<VMFunctionEnvironment>,
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
    /// Panics if both handles point to the same object. This operation
    /// is not supported for VMExceptionObj.
    pub fn get_2_mut<T: StoreObject>(
        &mut self,
        a: InternalStoreHandle<T>,
        b: InternalStoreHandle<T>,
    ) -> (&mut T, &mut T) {
        T::list_mut(self)
            .pair_mut(a.idx, b.idx)
            .expect("get_2_mut not supported for this type")
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

impl<T: StoreObject> fmt::Debug for StoreHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoreHandle")
            .field("id", &self.id)
            .field("internal", &self.internal.index())
            .finish()
    }
}

impl<T: StoreObject> PartialEq for StoreHandle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.internal == other.internal
    }
}

impl<T: StoreObject> Eq for StoreHandle<T> {}

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

impl<T: StoreObject> InternalStoreHandle<T> {
    /// Moves the given object into a context and returns a handle to it.
    pub fn new(ctx: &mut StoreObjects, val: T) -> Self {
        let idx = T::list_mut(ctx).append(val);
        Self {
            idx,
            marker: PhantomData,
        }
    }

    /// Returns a reference to the object that this handle points to.
    pub fn get<'a>(&self, ctx: &'a StoreObjects) -> &'a T {
        T::list(ctx).get(self.idx)
    }

    /// Returns a mutable reference to the object that this handle points to.
    pub fn get_mut<'a>(&self, ctx: &'a mut StoreObjects) -> &'a mut T {
        T::list_mut(ctx).get_mut(self.idx)
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
