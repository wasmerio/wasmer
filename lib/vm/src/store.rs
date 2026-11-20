use crate::{
    CurrentThread, VMExceptionObj, VMExternObj, VMFunction, VMFunctionEnvironment, VMGlobal,
    VMInstance, VMMemory, VMTable, VMTag,
};
use core::slice::Iter;
use std::{
    cell::{Cell, UnsafeCell},
    fmt,
    marker::PhantomData,
    num::NonZeroUsize,
    ptr::NonNull,
};
use wasmer_types::StoreId;

/// What a call into a store leaves for the host functions among its objects
/// while it runs (see [`StoreObjects::slot`]).
pub struct StoreSlot {
    store: Cell<*mut u8>,
    thread: Cell<Option<CurrentThread>>,
}

impl StoreSlot {
    /// The store of the innermost call into it, null outside of any: the
    /// embedder writes it before the call enters Wasm and puts the previous
    /// value back when the call returns.
    #[inline]
    pub fn store(&self) -> &Cell<*mut u8> {
        &self.store
    }

    /// The thread that call runs on, which the embedder writes and puts back
    /// with the store. A host function the call reaches is on that thread, and
    /// can take it from here instead of looking it up (see
    /// [`host_call_in_place_on`](crate::host_call_in_place_on)).
    #[inline]
    pub fn thread(&self) -> &Cell<Option<CurrentThread>> {
        &self.thread
    }
}

/// The allocation behind [`StoreObjects::slot`].
struct StoreSlotBox(NonNull<StoreSlot>);

impl Default for StoreSlotBox {
    fn default() -> Self {
        let slot: &mut StoreSlot = Box::leak(Box::new(StoreSlot {
            store: Cell::new(std::ptr::null_mut()),
            thread: Cell::new(None),
        }));
        Self(NonNull::from(slot))
    }
}

impl Drop for StoreSlotBox {
    fn drop(&mut self) {
        // SAFETY: leaked from a box by `default`, and freed only here.
        drop(unsafe { Box::from_raw(self.0.as_ptr()) });
    }
}

impl fmt::Debug for StoreSlotBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("StoreSlot").field(&self.0).finish()
    }
}

/// Trait to represent an object managed by a context. This is implemented on
/// the VM types managed by the context.
pub trait StoreObject: Sized {
    /// List the objects in the store.
    fn list(ctx: &StoreObjects) -> &Vec<Self>;

    /// List the objects in the store, mutably.
    ///
    /// # Safety
    ///
    /// The objects stay with the objects they were added to: none may be
    /// moved into another [`StoreObjects`], and none that something points
    /// into may be removed or replaced. The host functions among the objects
    /// keep a pointer to their slot ([`StoreObjects::slot`]), which these
    /// objects free when they are dropped, and instances keep pointers into
    /// the functions, memories, tables, globals and tags they import. Adding
    /// objects is fine, and so is replacing a function environment: its
    /// handles check the type of what they find.
    unsafe fn list_mut(ctx: &mut StoreObjects) -> &mut Vec<Self>;
}

macro_rules! impl_context_object {
    ($($field:ident => $ty:ty,)*) => {
        $(
            impl StoreObject for $ty {
                fn list(ctx: &StoreObjects) -> &Vec<Self> {
                    &ctx.$field
                }
                unsafe fn list_mut(ctx: &mut StoreObjects) -> &mut Vec<Self> {
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
    function_environments => VMFunctionEnvironment,
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
    exceptions: Vec<VMExceptionObj>,
    tags: Vec<VMTag>,
    function_environments: Vec<VMFunctionEnvironment>,
    /// Last, so that it outlives every object above.
    slot: StoreSlotBox,
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
        exceptions: Vec<VMExceptionObj>,
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
            slot: StoreSlotBox::default(),
        }
    }

    /// A [`StoreSlot`] in a heap allocation of its own, owned by these
    /// objects and freed with them, after every object in them.
    ///
    /// The embedder uses it to hand its store, and the thread a call into the
    /// store runs on, to the host functions among the objects while the call
    /// runs. It belongs to the objects rather than to the store around them,
    /// so that it goes wherever the objects go, together with the host
    /// functions that keep a pointer to it. Being a separate allocation, it is
    /// not covered by borrows of the objects.
    #[inline]
    pub fn slot(&self) -> NonNull<StoreSlot> {
        self.slot.0
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
    pub fn get_2_mut<T: StoreObject>(
        &mut self,
        a: InternalStoreHandle<T>,
        b: InternalStoreHandle<T>,
    ) -> (&mut T, &mut T) {
        assert_ne!(a.index(), b.index());
        // SAFETY: the objects stay in the list.
        let list = unsafe { T::list_mut(self) };
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
    /// Safety: the caller should check that the raw value is compatible
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

/// The panic of a [`StoreHandle`] used with another store's objects. Out of
/// line, and given the ids by value: an `assert_eq!` in place would take their
/// addresses, which keeps whatever holds the handle in memory on every lookup.
#[cold]
#[inline(never)]
#[track_caller]
fn wrong_context(handle: StoreId, ctx: StoreId) -> ! {
    panic!(
        "object used with the wrong context: the handle is of {handle:?}, the objects are {ctx:?}"
    )
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
    #[inline]
    pub fn get<'a>(&self, ctx: &'a StoreObjects) -> &'a T {
        if self.id != ctx.id {
            wrong_context(self.id, ctx.id);
        }
        self.internal.get(ctx)
    }

    /// Returns a mutable reference to the object that this handle points to.
    #[inline]
    pub fn get_mut<'a>(&self, ctx: &'a mut StoreObjects) -> &'a mut T {
        if self.id != ctx.id {
            wrong_context(self.id, ctx.id);
        }
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
        std::mem::size_of_val(self)
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
        // SAFETY: only adds an object.
        let list = unsafe { T::list_mut(ctx) };
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
        // SAFETY: the objects stay in the list.
        let list = unsafe { T::list_mut(ctx) };
        &mut list[self.idx.get() - 1]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn objects() -> StoreObjects {
        StoreObjects::new(
            StoreId::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    }

    #[test]
    #[should_panic(expected = "object used with the wrong context")]
    fn a_handle_used_with_other_objects_panics() {
        let mut ours = objects();
        let theirs = objects();
        let handle = StoreHandle::new(&mut ours, VMFunctionEnvironment::new(1u8));
        handle.get(&theirs);
    }

    #[test]
    fn a_slot_starts_empty() {
        let objects = objects();
        let slot = unsafe { objects.slot().as_ref() };
        assert!(slot.store().get().is_null());
        assert!(slot.thread().get().is_none());
    }
}
