//! Thread-local storage for storing the current store context,
//! i.e. the currently active [`Store`]s. When
//! a function is called, a pointer to the [`StoreInner`] in placed
//! inside the store context so it can be retrieved when needed.
//! This lets code that needs access to the store get it with
//! just the store ID.
//!
//! The currently active store context can be a sync or async
//! context.
//!
//! For sync contexts, we just store a raw pointer
//! to the `StoreInner`, which is owned by the embedder's stack.
//!
//! For async contexts, we store a write guard taken from the
//! `StoreAsync`; This achieves two goals:
//!   * Makes the `StoreAsync` available
//!     to whoever needs it, including when code needs to spawn
//!     new coroutines
//!   * Makes sure a write lock is held on the store as long as
//!     the context is active, preventing other tasks from
//!     accessing the store concurrently.
//!
//! We maintain a stack because it is technically possible to
//! have nested `Function::call` invocations that use different
//! stores, such as:
//!     call(store1, func1) -> wasm code -> imported func ->
//!     call(store2, func2)
//!
//! Note that this stack is maintained by both function
//! calls and the async_runtime to reflect the exact WASM
//! functions running on a given thread at any moment in
//! time. If a function suspends, its store context is
//! cleared and later reinstalled when it resumes. This lets
//! us use thread-local storage for the context without
//! requiring that async tasks are tied to specific threads.
//!
//! When something needs the "currently active" store context,
//! they will only look at the top entry in the stack. It is
//! always an error for code to try to access a store that's
//! "paused", i.e. not the top entry. This should be impossible
//! due to how the function call code is structured, but we
//! guard against it anyway.
//!
//! On the `sys` backend, a plain store (one not used from
//! coroutines) skips the stack: a call into it leaves its pointer
//! in the slot of its objects (see [`SlotRef`]), where its host
//! functions find it without consulting any thread-local state. The stack
//! is still what [`StoreMut::parked`] and
//! [`Store::with_current`](crate::Store::with_current) use.

use std::{
    borrow::BorrowMut,
    cell::{Cell, RefCell, UnsafeCell},
    mem::MaybeUninit,
    ptr::NonNull,
};

#[cfg(feature = "experimental-async")]
use crate::{LocalRwLock, LocalRwLockWriteGuard};

use super::{AsStoreMut, AsStoreRef, StoreInner, StoreMut, StoreRef};

use wasmer_types::StoreId;

enum StoreContextEntry {
    Sync(*mut StoreInner),

    #[cfg(feature = "experimental-async")]
    Async {
        guard: LocalRwLockWriteGuard<Box<StoreInner>>,
        /// The store, taken from `guard` once, when the entry is made. Every
        /// borrow the entry hands out comes from this pointer, rather than
        /// from a shared reference it would then write through. A call from
        /// a host function lends its own borrow here while it runs (see
        /// [`StoreContext::install`]).
        ptr: *mut StoreInner,
        /// Tells this entry from those installed for the same store when the
        /// call resumes, whose `ptr` a call lent before is not restored into.
        serial: u64,
    },
}

impl StoreContextEntry {
    #[cfg(feature = "experimental-async")]
    fn new_async(mut guard: LocalRwLockWriteGuard<Box<StoreInner>>) -> Self {
        // A raw pointer to the box's contents, taken once: every borrow the
        // entry hands out comes from it, but while a host function lends it
        // its own (see `install`). Unlike those of a synchronous entry, they
        // are siblings of one another, and of the borrows taken through the
        // entry made when the call resumes: none of them nests in another.
        let ptr: *mut StoreInner = &raw mut **guard;
        let serial = ASYNC_ENTRY_SERIAL.get();
        ASYNC_ENTRY_SERIAL.set(serial.wrapping_add(1));
        Self::Async { guard, ptr, serial }
    }

    fn as_ptr(&self) -> *mut StoreInner {
        match self {
            Self::Sync(ptr) => *ptr,
            #[cfg(feature = "experimental-async")]
            Self::Async { ptr, .. } => *ptr,
        }
    }
}

pub(crate) struct StoreContext {
    id: StoreId,

    // StoreContexts can be used recursively when Function::call
    // is used in an imported function. In the scenario, we're
    // essentially passing a mutable borrow of the store into
    // Function::call. However, entering the WASM code loses the
    // reference, and it needs to be re-acquired from the
    // StoreContext. This is why we use an UnsafeCell to allow
    // multiple mutable references to the StoreMut; we do however
    // keep track of how many borrows there are so we don't drop
    // it prematurely.
    borrow_count: u32,
    entry: UnsafeCell<StoreContextEntry>,
    /// For the entry of a call that a coroutine run by a `Cothread` made into
    /// a plain store: the store's slot, hidden while the entry is on the
    /// thread (see `StoreContext::enter_in_cothread`).
    #[cfg(feature = "unsafe-cothread")]
    hidden: Option<HiddenSlot>,
}

/// The slot of a store, and what it held before an entry of the context
/// stack hid it: see `StoreContext::enter_in_cothread`.
#[cfg(feature = "unsafe-cothread")]
struct HiddenSlot {
    slot: SlotRef,
    store: *mut StoreInner,
    thread: Option<wasmer_vm::CurrentThread>,
}

#[cfg(feature = "unsafe-cothread")]
impl HiddenSlot {
    /// Empties `slot`, keeping what it held.
    fn hide(slot: SlotRef) -> Self {
        Self {
            slot,
            store: slot.cell().replace(std::ptr::null_mut()),
            thread: slot.slot().thread().replace(None),
        }
    }

    /// Empties the slot again, keeping what it holds now: for an entry that
    /// goes back on the thread.
    fn hide_again(&mut self) {
        *self = Self::hide(self.slot);
    }

    /// Puts back what the slot held.
    fn restore(&self) {
        self.slot.cell().set(self.store);
        self.slot.slot().thread().set(self.thread);
    }

    /// Whether a call into the store runs through the slot.
    fn is_held(&self) -> bool {
        self.slot.get().is_some()
    }
}

pub(crate) struct StorePtrWrapper {
    store_ptr: *mut StoreInner,
    /// The store's id, so that giving the borrow back does not use
    /// `store_ptr`, which may have been invalidated since: lent by a host
    /// function whose call suspended (see [`StoreContext::install`]).
    id: StoreId,
    /// Whether the borrow is counted in an entry of the context stack, which
    /// dropping the wrapper gives it back to. A borrow taken from a store's
    /// slot is not counted: see [`SlotRef`].
    counted: bool,
}

#[cfg(feature = "experimental-async")]
pub(crate) struct StoreAsyncGuardWrapper {
    pub(crate) guard: *mut LocalRwLockWriteGuard<Box<StoreInner>>,
    /// The entry's pointer to the store, to borrow it through rather than
    /// through `guard`.
    pub(crate) store_ptr: *mut StoreInner,
    /// The store's id: see [`StorePtrWrapper`].
    id: StoreId,
}

pub(crate) struct StorePtrPauseGuard {
    store_id: StoreId,
    ptr: *mut StoreInner,
    ref_count_decremented: bool,
}

#[cfg(feature = "experimental-async")]
pub(crate) enum GetStoreAsyncGuardResult {
    Ok(StoreAsyncGuardWrapper),
    NotAsync(StorePtrWrapper),
    NotInstalled,
}

pub(crate) struct ForcedStoreInstallGuard {
    store_id: StoreId,
}

pub(crate) enum StoreInstallGuard {
    /// The guard pushed an entry of its own, which it pops when dropped.
    Pushed(StoreId),
    /// The guard pushed nothing: an async context already holds the store
    /// `id`, and the pointer of its entry `serial` was lent, and is restored
    /// to `previous` — see [`StoreContext::install`].
    #[cfg(feature = "experimental-async")]
    Lent {
        id: StoreId,
        serial: u64,
        previous: *mut StoreInner,
    },
}

thread_local! {
    static STORE_CONTEXT_STACK: RefCell<Vec<StoreContext>> = const { RefCell::new(Vec::new()) };
}

/// Whether the innermost call running on this thread is a call into a plain
/// store (see [`SlotRef`]), which has no entry on the context stack:
/// [`StoreContext::try_get_current_unborrowed`] must not lend out the entry
/// below it, as the call's host functions hold the borrow of their own store.
/// [`StoreMut::parked`] clears it for its closure.
///
/// The flag is `wasmer_vm::CurrentThread::slot_call`, in the VM's thread
/// state: a call looks the thread up once to set it and to enter Wasm. Only
/// the `sys` backend has plain stores.
#[inline]
fn inside_plain_call() -> bool {
    #[cfg(feature = "sys")]
    {
        wasmer_vm::CurrentThread::get().slot_call().get()
    }
    #[cfg(not(feature = "sys"))]
    {
        false
    }
}

/// The bits of the context mode of a `sys` store (see
/// [`BackendStore::context_mode`](crate::BackendStore::context_mode)). A store
/// with none of them set is a plain store, whose calls go through its slot
/// (see [`SlotRef`]), except those of a coroutine a `Cothread` runs.
#[cfg(feature = "sys")]
pub(crate) mod context_mode {
    /// The store lives in a `StoreAsync`, where `call_async` coroutines that
    /// suspend and resume in any order share it.
    pub(crate) const ASYNC: u8 = 1;
}

/// Where the host functions of a plain store find the store while a call into
/// it runs: a pointer to the slot of the [`StoreObjects`](crate::StoreObjects)
/// they live in (see `wasmer_vm::StoreObjects::slot`).
///
/// A call into a plain store writes the pointer it derived from its caller's
/// borrow into the slot of the store's objects and puts the previous value
/// back when it returns, so the slot always holds the pointer of the innermost
/// call into the store (null outside of any): a host function's borrow is a
/// child of the borrow that called into the guest, as nested calls need (see
/// [`StoreContext::install`] for why that matters). A store that is entered
/// through the thread's context stack instead, because it is used from
/// `call_async` coroutines (see [`context_mode`]), leaves its slot null, and
/// its host functions fall back to the stack. So does a call that a coroutine
/// run by a `Cothread` makes, whatever the store: see
/// `StoreContext::enter_in_cothread`.
///
/// The slot is a separate allocation that host functions keep a pointer to.
/// Inside `StoreInner` it would be covered by the caller's borrow of the
/// store, and reading it through a pointer kept elsewhere would be a foreign
/// access to that borrow. It belongs to the objects, not to the store around
/// them, because the objects, and the host functions in them, can be moved to
/// another store (`objects_mut` hands them out): wherever they go, the slot
/// goes with them, and a call into that store writes the slot its host
/// functions read. The objects free it after the host functions, so it
/// outlives them.
#[cfg(feature = "sys")]
#[derive(Clone, Copy)]
pub(crate) struct SlotRef(NonNull<wasmer_vm::StoreSlot>);

#[cfg(feature = "sys")]
impl SlotRef {
    /// The slot of `objects`, which host functions made among them keep.
    #[inline]
    pub(crate) fn of(objects: &crate::StoreObjects) -> Self {
        Self(objects.as_sys().slot())
    }

    /// The address of the slot.
    #[inline]
    pub(crate) fn as_ptr(self) -> NonNull<wasmer_vm::StoreSlot> {
        self.0
    }

    /// The slot at `ptr`.
    ///
    /// # Safety
    /// `ptr` must be the address of a slot, from [`Self::as_ptr`].
    #[inline]
    pub(crate) unsafe fn from_ptr(ptr: NonNull<wasmer_vm::StoreSlot>) -> Self {
        Self(ptr)
    }

    #[inline]
    fn slot(&self) -> &wasmer_vm::StoreSlot {
        // SAFETY: valid for as long as the objects that own it, and with them
        // the host functions that keep it, live; only `Cell`'s shared access
        // is used.
        unsafe { self.0.as_ref() }
    }

    #[inline]
    fn cell(&self) -> &Cell<*mut StoreInner> {
        // SAFETY: the same pointer, typed as what this crate writes into it.
        unsafe { &*std::ptr::from_ref(self.slot().store()).cast::<Cell<*mut StoreInner>>() }
    }

    /// The thread the innermost call into the store runs on, if one is
    /// running through the slot: the calling thread, for a host function
    /// that call reaches.
    #[inline]
    pub(crate) fn thread(&self) -> Option<wasmer_vm::CurrentThread> {
        self.slot().thread().get()
    }

    /// The store of the innermost call into it, if one is running.
    #[inline]
    pub(crate) fn get(&self) -> Option<NonNull<StoreInner>> {
        NonNull::new(self.cell().get())
    }
}

/// What a call into a store undoes when it returns: see
/// [`StoreContext::enter`].
#[cfg(feature = "sys")]
pub(crate) enum StoreEntryGuard {
    /// The call wrote its pointer into the store's slot.
    Slot {
        slot: SlotRef,
        /// The slot's store and thread before the call.
        saved: *mut StoreInner,
        saved_thread: Option<wasmer_vm::CurrentThread>,
        /// The calling thread, and its flag of whether a plain call is
        /// innermost (see [`inside_plain_call`]) before the call.
        thread: wasmer_vm::CurrentThread,
        saved_inside_plain_call: bool,
    },
    /// The call went through the context stack.
    Stack(StoreInstallGuard),
}

#[cfg(feature = "sys")]
impl StoreEntryGuard {
    /// See [`StoreInstallGuard::pause`]. A call through the slot has no
    /// borrow to pause: it is the slot's alone, and nobody counts it.
    ///
    /// # Safety
    /// See [`StoreContext::pause`].
    #[inline]
    pub(crate) unsafe fn pause(&self) -> Option<StorePtrPauseGuard> {
        match self {
            Self::Slot { .. } => None,
            Self::Stack(guard) => unsafe { guard.pause() },
        }
    }
}

#[cfg(feature = "sys")]
impl Drop for StoreEntryGuard {
    #[inline]
    fn drop(&mut self) {
        match self {
            // Nothing here can panic: this runs while unwinding too, and puts
            // the slot back for a host function further up that catches the
            // panic.
            Self::Slot {
                slot,
                saved,
                saved_thread,
                thread,
                saved_inside_plain_call,
            } => {
                slot.cell().set(*saved);
                slot.slot().thread().set(*saved_thread);
                // This thread's (the guard is not `Send`), whose state
                // outlives the call.
                thread.slot_call().set(*saved_inside_plain_call);
            }
            // Dropped by its own `Drop`.
            Self::Stack(_) => {}
        }
    }
}

/// Lifts the flag of [`inside_plain_call`] while a borrow is parked: the
/// frames inside see the parked entry, which they may borrow.
pub(crate) struct LendGuard {
    saved: bool,
}

impl LendGuard {
    pub(crate) fn new() -> Self {
        #[cfg(feature = "sys")]
        let saved = wasmer_vm::CurrentThread::get().slot_call().replace(false);
        #[cfg(not(feature = "sys"))]
        let saved = false;
        Self { saved }
    }
}

impl Drop for LendGuard {
    fn drop(&mut self) {
        #[cfg(feature = "sys")]
        if let Some(thread) = wasmer_vm::CurrentThread::try_get() {
            thread.slot_call().set(self.saved);
        }
    }
}

#[cfg(feature = "experimental-async")]
thread_local! {
    /// The `serial` of the next async entry made on this thread.
    static ASYNC_ENTRY_SERIAL: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(feature = "unsafe-cothread")]
thread_local! {
    /// How many entries of the store context stack lie beneath the
    /// [`Cothread`] that runs: they belong to the calls of whatever resumed
    /// it, which the coroutine does not run in.
    static COTHREAD_BASE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl StoreContext {
    /// Whether `id`'s entry is the active one.
    pub(crate) fn is_active(id: StoreId) -> bool {
        STORE_CONTEXT_STACK.with(|cell| {
            let stack = cell.borrow();
            stack.last().is_some_and(|ctx| ctx.id == id)
        })
    }

    /// Whether `id` has an entry on the thread's stack, active or not: a call
    /// on this thread, beneath whatever runs, holds the store.
    #[cfg(all(
        feature = "sys",
        any(feature = "experimental-async", feature = "unsafe-cothread")
    ))]
    pub(crate) fn is_held(id: StoreId) -> bool {
        STORE_CONTEXT_STACK.with(|cell| cell.borrow().iter().any(|ctx| ctx.id == id))
    }

    /// Whether the thread still has what unwinding a call suspended in Wasm
    /// needs of it: false once its thread locals are being destroyed, as it
    /// exits. A call suspended then is forgotten rather than unwound.
    #[cfg(all(
        feature = "sys",
        any(feature = "experimental-async", feature = "unsafe-cothread")
    ))]
    pub(crate) fn thread_can_unwind() -> bool {
        STORE_CONTEXT_STACK.try_with(|_| ()).is_ok()
            && wasmer_vm::ActivationState::thread_is_usable()
    }

    /// Puts back the slot the entry hid, as it leaves the thread.
    #[inline]
    fn leave(&self) {
        #[cfg(feature = "unsafe-cothread")]
        if let Some(hidden) = &self.hidden {
            hidden.restore();
        }
    }

    fn push(id: StoreId, entry: StoreContextEntry) {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            stack.push(Self {
                id,
                borrow_count: 0,
                entry: UnsafeCell::new(entry),
                #[cfg(feature = "unsafe-cothread")]
                hidden: None,
            });
        })
    }

    /// Returns true if no call into a store is running on this thread: there
    /// are no active store context entries, and no call into a plain store
    /// (which has none) is innermost.
    pub(crate) fn is_empty() -> bool {
        !inside_plain_call()
            && STORE_CONTEXT_STACK.with(|cell| {
                let stack = cell.borrow();
                stack.is_empty()
            })
    }

    /// The write guard ensures this is the only reference to the store,
    /// so installation can never fail.
    #[cfg(feature = "experimental-async")]
    pub(crate) fn install_async(
        guard: LocalRwLockWriteGuard<Box<StoreInner>>,
    ) -> ForcedStoreInstallGuard {
        let store_id = guard.objects.id();
        Self::push(store_id, StoreContextEntry::new_async(guard));
        ForcedStoreInstallGuard { store_id }
    }

    /// Whether the entry on top of the thread's stack, the store that is
    /// running, is held by an async context. A store entered synchronously
    /// cannot be suspended, not even from a `call_async` coroutine further
    /// down the stack: its caller holds a borrow of it across the call.
    #[cfg(feature = "experimental-async")]
    pub(crate) fn top_is_async() -> bool {
        STORE_CONTEXT_STACK.with(|cell| {
            cell.borrow().last().is_some_and(|top| {
                matches!(
                    unsafe { top.entry.get().as_ref().unwrap() },
                    StoreContextEntry::Async { .. }
                )
            })
        })
    }

    /// Makes `store_ptr` the store its host functions use until the returned
    /// guard is dropped: what a call does before it enters the guest.
    ///
    /// A plain store gets the pointer in its slot (see [`SlotRef`]); a store
    /// used from coroutines gets an entry on the context stack, as
    /// [`Self::install`] describes.
    ///
    /// # Safety
    /// See [`Self::install`].
    #[cfg(feature = "sys")]
    #[inline]
    pub(crate) unsafe fn enter(store_ptr: *mut StoreInner) -> StoreEntryGuard {
        // Read through the pointer without making a mutable reference of it:
        // the host functions' borrows are derived from this very pointer.
        if unsafe { (*store_ptr).store.context_mode() } != 0 {
            return StoreEntryGuard::Stack(unsafe { Self::install(store_ptr) });
        }
        unsafe { Self::enter_slot(wasmer_vm::CurrentThread::get(), store_ptr) }
    }

    /// [`Self::enter`] for a call that enters Wasm on `thread` right after,
    /// which it looked up already: the calling thread's.
    ///
    /// # Safety
    /// See [`Self::install`].
    #[cfg(feature = "sys")]
    #[inline]
    pub(crate) unsafe fn enter_on(
        thread: wasmer_vm::CurrentThread,
        store_ptr: *mut StoreInner,
    ) -> StoreEntryGuard {
        if unsafe { (*store_ptr).store.context_mode() } != 0 {
            return StoreEntryGuard::Stack(unsafe { Self::install(store_ptr) });
        }
        unsafe { Self::enter_slot(thread, store_ptr) }
    }

    /// [`Self::enter`] for a plain store.
    #[cfg(feature = "sys")]
    #[inline]
    unsafe fn enter_slot(
        thread: wasmer_vm::CurrentThread,
        store_ptr: *mut StoreInner,
    ) -> StoreEntryGuard {
        #[cfg(feature = "unsafe-cothread")]
        if thread.cothread().get() {
            return unsafe { Self::enter_in_cothread(store_ptr) };
        }
        // The slot of the objects the store holds now, where the host
        // functions among them look.
        let slot = SlotRef::of(unsafe { &(*store_ptr).objects });
        let saved = slot.cell().replace(store_ptr);
        let saved_thread = slot.slot().thread().replace(Some(thread));
        let saved_inside_plain_call = thread.slot_call().replace(true);
        StoreEntryGuard::Slot {
            slot,
            saved,
            saved_thread,
            thread,
            saved_inside_plain_call,
        }
    }

    /// [`Self::enter`] for a plain store, from a coroutine that a `Cothread`
    /// runs: through the context stack, as for a store used from `call_async`
    /// coroutines. The `Cothread` takes the entries of the coroutine's calls
    /// off the thread when it suspends, and puts them back when it is
    /// resumed, in whatever order coroutines are; calls through the slot
    /// would put back each other's values.
    ///
    /// A call beneath the coroutine may hold the store through its slot, and
    /// have lent it to the coroutine, as to a call from a host function. The
    /// entry hides the slot while it is on the thread, as an entry shadows
    /// one beneath it: the host functions of the coroutine's call find the
    /// entry rather than the borrow of the call beneath. The `Cothread` puts
    /// back what the slot held when it takes the entry off the thread, and a
    /// store whose slot holds a call when the entry is to go back on is one
    /// in use (see `CothreadState::install`).
    ///
    /// # Safety
    /// See [`Self::install`].
    #[cfg(feature = "unsafe-cothread")]
    #[cold]
    #[inline(never)]
    unsafe fn enter_in_cothread(store_ptr: *mut StoreInner) -> StoreEntryGuard {
        let slot = SlotRef::of(unsafe { &(*store_ptr).objects });
        let guard = unsafe { Self::install(store_ptr) };
        if let StoreInstallGuard::Pushed(_) = guard {
            STORE_CONTEXT_STACK.with(|cell| {
                let mut stack = cell.borrow_mut();
                let top = stack.last_mut().expect("the entry just pushed");
                top.hidden = Some(HiddenSlot::hide(slot));
            });
        }
        StoreEntryGuard::Stack(guard)
    }

    /// The borrow a host function of the store `id` takes, whose slot is
    /// `slot`: from the slot if a call into the store is running through it,
    /// and from the context stack otherwise, as [`Self::get_current`] does.
    ///
    /// # Safety
    /// See [`Self::get_current`].
    #[cfg(feature = "sys")]
    #[inline]
    pub(crate) unsafe fn get_current_in(slot: SlotRef, id: StoreId) -> StorePtrWrapper {
        match slot.get() {
            Some(store_ptr) => {
                debug_assert_eq!(
                    unsafe { store_ptr.as_ref() }.objects.id(),
                    id,
                    "a store's slot holds another store"
                );
                StorePtrWrapper {
                    store_ptr: store_ptr.as_ptr(),
                    id,
                    counted: false,
                }
            }
            None => unsafe { Self::get_current(id) },
        }
    }

    /// [`Self::get_current_transient`] for a host function of the store `id`,
    /// whose slot is `slot`: see [`Self::get_current_in`].
    ///
    /// # Safety
    /// See [`Self::get_current_transient`].
    #[cfg(feature = "sys")]
    #[inline]
    pub(crate) unsafe fn get_current_transient_in(slot: SlotRef, id: StoreId) -> *mut StoreInner {
        match slot.get() {
            Some(store_ptr) => store_ptr.as_ptr(),
            None => unsafe { Self::get_current_transient(id) },
        }
    }

    /// Install `store_ptr` as this thread's executing store until the returned
    /// guard is dropped.
    ///
    /// A store already executing here gets a *second* entry rather than
    /// re-using its first: the entries differ in which borrow their pointer was
    /// derived from, and that difference is load-bearing. Every re-acquisition
    /// ([`Self::get_current`] and friends) reborrows the top entry's pointer,
    /// so re-using an outer entry would make the inner borrow a *sibling* of
    /// the one the caller is still holding — creating it invalidates the
    /// caller's, and the caller's next use of its own store is undefined
    /// behaviour. Installing the pointer the caller just derived from its live
    /// borrow makes the inner borrow a child of it instead, which is what lets
    /// the caller carry on once the nested call returns. See the
    /// `borrow_provenance` tests, which fail under Miri if this re-uses the
    /// outer entry.
    ///
    /// An async context is the exception, and pushes nothing: its entry holds
    /// the write guard, which async host functions reach by inspecting the
    /// active entry, and through which the call suspends. A second entry
    /// would hide it from them. Instead, the async entry's pointer is
    /// replaced with `store_ptr` until the guard is dropped, which nests the
    /// borrows taken through it in the caller's, as a second entry would: a
    /// host function that calls its guest back goes on with its own store
    /// afterwards. A call that suspends meanwhile gives the store up, and
    /// resumes with a new entry, which keeps its own pointer. That leaves out
    /// a host function that goes on with the borrow it held across a call that
    /// suspended: whoever used the store meanwhile did not borrow it from that
    /// borrow. One beneath a `Cothread` that was resumed since is shadowed all
    /// the same: the coroutine's calls need an entry of their own, which it
    /// takes with it when it suspends, and the call that holds the guard
    /// cannot be suspended from the coroutine's stack.
    ///
    /// # Safety
    /// `store_ptr` must be derived from a mutable borrow of the store that
    /// stays alive, and unreachable to every frame on this thread, until the
    /// guard is dropped.
    pub(crate) unsafe fn install(store_ptr: *mut StoreInner) -> StoreInstallGuard {
        let store_id = unsafe { store_ptr.as_ref().unwrap().objects.id() };
        // One access to the thread-local for the check and the push: this
        // runs on every entry into Wasm.
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            #[cfg(feature = "unsafe-cothread")]
            let depth = stack.len();
            if let Some(top) = stack.last_mut()
                && top.id == store_id
            {
                let active = top.entry.get_mut();
                // Nesting changes a pointer's provenance, never which store
                // it addresses, so the same store must arrive at the same
                // address.
                debug_assert!(
                    active.as_ptr() == store_ptr,
                    "Store context pointer mismatch"
                );
                // Not one beneath a `Cothread` that was resumed since: see
                // above.
                #[cfg(feature = "unsafe-cothread")]
                let above_cothread = depth > COTHREAD_BASE.get();
                #[cfg(not(feature = "unsafe-cothread"))]
                let above_cothread = true;
                #[cfg(feature = "experimental-async")]
                if above_cothread && let StoreContextEntry::Async { ptr, serial, .. } = active {
                    return StoreInstallGuard::Lent {
                        id: store_id,
                        serial: *serial,
                        previous: std::mem::replace(ptr, store_ptr),
                    };
                }
                let _ = above_cothread;
            }
            stack.push(Self {
                id: store_id,
                borrow_count: 0,
                entry: UnsafeCell::new(StoreContextEntry::Sync(store_ptr)),
                #[cfg(feature = "unsafe-cothread")]
                hidden: None,
            });
            StoreInstallGuard::Pushed(store_id)
        })
    }

    /// "Pause" one borrow of the store context.
    ///
    /// # Safety
    /// Code must ensure it does not use the StorePtrWrapper or
    /// StoreAsyncGuardWrapper that it owns, or any StoreRef/StoreMut
    /// derived from them, while the store context is paused.
    ///
    /// The safe, correct use-case for this method is to
    /// pause the store context while executing WASM code, which
    /// cannot use the store context directly. This allows an async
    /// context to uninstall the store context when suspending if it's
    /// called from a sync imported function. The imported function
    /// will have borrowed the store context in its trampoline, which
    /// will prevent the async context from uninstalling the store.
    /// However, since the imported function passes a mutable borrow
    /// of its store into `Function::call`, it will expect the store
    /// to change before the call returns.
    ///
    /// Entries into Wasm go through [`StoreInstallGuard::pause`], which
    /// calls this only for an async context's entry.
    // Without async contexts, only the tests pause an entry.
    #[cfg_attr(not(feature = "experimental-async"), allow(dead_code))]
    unsafe fn pause(id: StoreId) -> StorePtrPauseGuard {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let top = stack
                .last_mut()
                .expect("No store context installed on this thread");
            assert_eq!(top.id, id, "Mismatched store context access");
            let ref_count_decremented = if top.borrow_count > 0 {
                top.borrow_count -= 1;
                true
            } else {
                false
            };
            StorePtrPauseGuard {
                store_id: id,
                ptr: unsafe { top.entry.get().as_ref().unwrap().as_ptr() },
                ref_count_decremented,
            }
        })
    }

    /// The active entry's store, if no frame on this thread's stack is holding
    /// a borrow of it.
    ///
    /// A non-zero borrow count means some frame further up still holds a
    /// `StoreMut` it can reach, so handing out a second one would alias it;
    /// the caller gets `None` instead. A frame that has lent its borrow out
    /// ([`StoreMut::parked`]) installed an entry of its own, whose count is
    /// zero until somebody takes it — which is how an imported function lends
    /// its store to code it calls into.
    ///
    /// A call into a plain store has no entry: while it is the innermost call,
    /// its host functions hold the borrow, and the answer is `None` too.
    pub(crate) fn try_get_current_unborrowed() -> Option<StorePtrWrapper> {
        if inside_plain_call() {
            return None;
        }
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let top = stack.last_mut()?;
            if top.borrow_count != 0 {
                return None;
            }
            top.borrow_count += 1;
            Some(StorePtrWrapper {
                store_ptr: unsafe { top.entry.get().as_mut().unwrap().as_ptr() },
                id: top.id,
                counted: true,
            })
        })
    }

    /// Safety: This method lets you borrow multiple mutable references
    /// to the currently active store context. The caller must ensure that:
    ///   * there is only one mutable reference alive, or
    ///   * all but one mutable reference are inaccessible and passed
    ///     into a function that lost the reference (e.g. into WASM code)
    ///
    /// The intended, valid use-case for this method is from within
    /// imported function trampolines.
    pub(crate) unsafe fn get_current(id: StoreId) -> StorePtrWrapper {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let top = stack
                .last_mut()
                .expect("No store context installed on this thread");
            assert_eq!(top.id, id, "Mismatched store context access");
            top.borrow_count += 1;
            StorePtrWrapper {
                store_ptr: unsafe { top.entry.get().as_mut().unwrap().as_ptr() },
                id,
                counted: true,
            }
        })
    }

    /// Safety: In addition to the safety requirements of [`Self::get_current`],
    /// the pointer returned from this function will become invalid if
    /// the store context is changed in any way (via installing or uninstalling
    /// a store context). The caller must ensure that the store context
    /// remains unchanged as long as the pointer is being accessed.
    pub(crate) unsafe fn get_current_transient(id: StoreId) -> *mut StoreInner {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let top = stack
                .last_mut()
                .expect("No store context installed on this thread");
            assert_eq!(top.id, id, "Mismatched store context access");
            unsafe { top.entry.get().as_mut().unwrap().as_ptr() }
        })
    }

    /// Safety: See [`Self::get_current`].
    pub(crate) unsafe fn try_get_current(id: StoreId) -> Option<StorePtrWrapper> {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let top = stack.last_mut()?;
            if top.id != id {
                return None;
            }
            top.borrow_count += 1;
            Some(StorePtrWrapper {
                store_ptr: unsafe { top.entry.get().as_mut().unwrap().as_ptr() },
                id: top.id,
                counted: true,
            })
        })
    }

    /// Safety: See [`Self::get_current`].
    #[cfg(feature = "experimental-async")]
    pub(crate) unsafe fn try_get_current_async(id: StoreId) -> GetStoreAsyncGuardResult {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let Some(top) = stack.last_mut() else {
                return GetStoreAsyncGuardResult::NotInstalled;
            };
            if top.id != id {
                return GetStoreAsyncGuardResult::NotInstalled;
            }
            top.borrow_count += 1;
            match unsafe { top.entry.get().as_mut().unwrap() } {
                StoreContextEntry::Async { guard, ptr, .. } => {
                    GetStoreAsyncGuardResult::Ok(StoreAsyncGuardWrapper {
                        guard: guard as *mut _,
                        store_ptr: *ptr,
                        id,
                    })
                }
                StoreContextEntry::Sync(ptr) => {
                    GetStoreAsyncGuardResult::NotAsync(StorePtrWrapper {
                        store_ptr: *ptr,
                        id,
                        counted: true,
                    })
                }
            }
        })
    }
}

/// A coroutine that an embedder switches itself and that can suspend while
/// it runs Wasm, kept with the thread state of the calls it is suspended in.
///
/// A call into Wasm keeps state on its thread while it runs: the entry its
/// host functions find the store through, and the trap handler the signal
/// handler consults (with `experimental-host-interrupt`, also the stores the
/// interrupt registry sees running). A coroutine suspended inside a call,
/// from a host function, has to take that state with it, or a call resumed in
/// another order than the calls entered Wasm finds another call's: its traps
/// are handled on the wrong stack. `Cothread` takes the coroutine's state
/// off the thread whenever control comes back from it, keeps it, and puts it
/// back for the next resume, as `Function::call_async` does for its own
/// coroutines.
///
/// The state belongs to the coroutine and to nothing else, so it cannot be
/// installed for another coroutine. Dropping a `Cothread` drops the
/// coroutine, which unwinds it if it is suspended, with its state installed;
/// forgetting one forgets the state with the coroutine. So the `Cothread`
/// must own the coroutine (see [`Cothread::new`]). A suspended coroutine that
/// cannot be unwound when its `Cothread` is dropped, because a store of the
/// calls it is suspended in is in use on the thread, or because the thread is
/// exiting, is forgotten instead, with its stack and its state.
///
/// Every coroutine that can suspend while a call into Wasm is on its stack
/// must be a `Cothread`, and be resumed only through [`Cothread::resume`].
/// A `Cothread` stays on the thread that made it.
///
/// The coroutine runs on no `Function::call_async` coroutine but those it
/// starts itself, also when it is resumed from a host function of one: an
/// async import it calls runs as one called synchronously does, and fails
/// with `YieldOutsideAsyncContext` if it has to wait. It enters a store that a
/// call beneath the resume holds, which it may be lent, as a call from a host
/// function does: with an entry of its own, whichever kind of call holds the
/// store, which goes with the coroutine when it suspends.
///
/// ```ignore
/// // Safety: the `Coroutine` is owned by the `Cothread`.
/// let mut cothread = unsafe {
///     Cothread::new(Coroutine::new(move |yielder, ()| {
///         // A host function of `run` suspends the coroutine with `yielder`.
///         run.call(&mut store, 0)
///     }))
/// };
/// unsafe { cothread.resume(|coroutine| coroutine.resume(())) };
/// ```
#[cfg(feature = "unsafe-cothread")]
pub struct Cothread<C> {
    coroutine: std::mem::ManuallyDrop<C>,
    state: CothreadState,
}

/// The thread state of a [`Cothread`]'s coroutine, while it is off the
/// thread.
#[cfg(feature = "unsafe-cothread")]
#[derive(Default)]
struct CothreadState {
    /// The entries the coroutine left on the store context stack when it
    /// last suspended, bottom first: those of the calls it is suspended in.
    /// The vector keeps its capacity while the entries are on the thread.
    entries: Vec<StoreContext>,
    /// The trap handler and active stores of those calls.
    activation: wasmer_vm::ActivationState,
    /// The `call_async` coroutine the thread runs on, as the coroutine left
    /// it: none until it starts one of its own. While the coroutine runs,
    /// that of whatever resumed it, which cannot be suspended from the
    /// coroutine's stack.
    #[cfg(feature = "experimental-async")]
    coroutine: crate::backend::sys::async_runtime::CoroutineSlot,
    /// Whether a call into a plain store is the innermost call of the
    /// coroutine (see [`inside_plain_call`]), as the coroutine left it: none
    /// until it first suspends, and then it starts with that of whatever
    /// resumed it, as a call from a host function does.
    slot_call: Option<bool>,
}

#[cfg(feature = "unsafe-cothread")]
impl CothreadState {
    fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.activation.is_empty()
    }

    /// Whether [`Self::install`] can put the state on the thread: none of
    /// its stores is in use there, and the thread is not exiting.
    fn can_install(&self) -> bool {
        StoreContext::thread_can_unwind()
            && !self.entries.iter().any(|entry| {
                StoreContext::is_held(entry.id)
                    || entry.hidden.as_ref().is_some_and(HiddenSlot::is_held)
            })
    }

    /// Puts the state on the thread until the returned guard is dropped.
    ///
    /// # Panics
    /// Panics if a store whose call the coroutine is suspended in is on the
    /// thread's context stack, or a call into it runs through its slot:
    /// something on the thread is using it.
    ///
    /// # Safety
    /// Until the guard is dropped, the thread runs nothing but the
    /// coroutine the state belongs to: the state holds pointers into its
    /// stack, and borrows of the stores its frames hold.
    unsafe fn install(&mut self) -> InstalledCothreadState<'_> {
        let base = STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            for entry in &self.entries {
                assert!(
                    !stack.iter().any(|ctx| ctx.id == entry.id)
                        && !entry.hidden.as_ref().is_some_and(HiddenSlot::is_held),
                    "a store the coroutine is suspended in is already in use on the \
                     current thread"
                );
            }
            // Bottom first, as the entries were made: each hides what the
            // slot holds now.
            for hidden in self.entries.iter_mut().filter_map(|e| e.hidden.as_mut()) {
                hidden.hide_again();
            }
            let base = stack.len();
            stack.extend(self.entries.drain(..));
            base
        });
        let outer_base = COTHREAD_BASE.replace(base);
        // The coroutine's calls go through the stack (see
        // `StoreContext::enter_in_cothread`), and it finds out whether a
        // plain call is innermost as it left it.
        let thread = wasmer_vm::CurrentThread::get();
        let outer_cothread = thread.cothread().replace(true);
        let outer_slot_call = thread.slot_call().get();
        if let Some(slot_call) = self.slot_call {
            thread.slot_call().set(slot_call);
        }
        // Safety: the first of a pair; the guard swaps again as soon as
        // control is back from the coroutine, however it comes back.
        unsafe { self.activation.swap() };
        #[cfg(feature = "experimental-async")]
        self.coroutine.swap();
        InstalledCothreadState {
            state: self,
            base,
            outer_base,
            outer_cothread,
            outer_slot_call,
        }
    }
}

/// Takes a [`CothreadState`] off the thread again when dropped.
#[cfg(feature = "unsafe-cothread")]
struct InstalledCothreadState<'a> {
    /// Holds the thread's own activation state while the coroutine's is
    /// installed.
    state: &'a mut CothreadState,
    /// Where the coroutine's entries start on the store context stack.
    base: usize,
    /// [`COTHREAD_BASE`] for whatever resumed the coroutine.
    outer_base: usize,
    /// The thread's flags of `wasmer_vm::CurrentThread` for whatever resumed
    /// the coroutine.
    outer_cothread: bool,
    outer_slot_call: bool,
}

#[cfg(feature = "unsafe-cothread")]
impl Drop for InstalledCothreadState<'_> {
    fn drop(&mut self) {
        // Safety: the second of the pair `install` started.
        unsafe { self.state.activation.swap() };
        #[cfg(feature = "experimental-async")]
        self.state.coroutine.swap();
        COTHREAD_BASE.set(self.outer_base);
        if let Some(thread) = wasmer_vm::CurrentThread::try_get() {
            thread.cothread().set(self.outer_cothread);
            let slot_call = thread.slot_call().replace(self.outer_slot_call);
            self.state.slot_call = Some(slot_call);
        }
        // Whatever is left above `base` belongs to the calls the coroutine
        // is suspended in. A coroutine that returned or unwound left nothing
        // behind: each of its calls took its entry and its trap handler off
        // on the way out. The slots the entries hid get back what they held,
        // top first, for whatever resumed the coroutine.
        let (base, entries) = (self.base, &mut self.state.entries);
        let taken = STORE_CONTEXT_STACK.try_with(|cell| {
            let mut stack = cell.try_borrow_mut().ok()?;
            (base <= stack.len()).then(|| {
                for entry in stack[base..].iter().rev() {
                    entry.leave();
                }
                entries.extend(stack.drain(base..))
            })
        });
        match taken {
            Ok(Some(())) => {}
            _ if std::thread::panicking() => {}
            _ => panic!(
                "Cothread: the store context stack is shorter than when the coroutine \
                 was resumed; the store context stack is corrupted"
            ),
        }
    }
}

#[cfg(feature = "unsafe-cothread")]
impl<C> Cothread<C> {
    /// Wraps a coroutine that has not run yet.
    ///
    /// # Safety
    /// `coroutine` must own the coroutine, and be the only way to resume it:
    /// dropping it must unwind the coroutine if it is suspended, and drop it
    /// there and then, as dropping a `corosensei::Coroutine` does. A borrow of
    /// a coroutine, or a handle shared with something else, would leave it
    /// suspended when the `Cothread` is dropped, with its thread state gone,
    /// for whoever unwinds it later to unwind without.
    pub unsafe fn new(coroutine: C) -> Self {
        Self {
            coroutine: std::mem::ManuallyDrop::new(coroutine),
            state: CothreadState::default(),
        }
    }

    /// The coroutine, to look at; it is resumed only by [`Self::resume`].
    pub fn get(&self) -> &C {
        &self.coroutine
    }

    /// Puts the coroutine's thread state on the thread, calls `resume`, and
    /// takes the state off again once `resume` returns or unwinds.
    ///
    /// # Panics
    /// Panics if a store whose call the coroutine is suspended in is in use
    /// on the current thread: another call on that store runs there, whether
    /// it is on the thread's context stack or holds the store through its
    /// slot.
    ///
    /// # Safety
    /// `resume` must resume the coroutine, on the thread that made the
    /// `Cothread`, and do nothing else with the stores of the calls the
    /// coroutine is suspended in: their entries are installed for the
    /// coroutine, whose frames hold borrows of them.
    pub unsafe fn resume<R>(&mut self, resume: impl FnOnce(&mut C) -> R) -> R {
        let _installed = unsafe { self.state.install() };
        resume(&mut self.coroutine)
    }
}

#[cfg(feature = "unsafe-cothread")]
impl<C> Drop for Cothread<C> {
    fn drop(&mut self) {
        // A coroutine suspended inside a call is unwound by dropping it,
        // which runs the frames of the call: they need their own state. If
        // that cannot be installed, the coroutine is forgotten: `install`
        // would panic, in a destructor.
        if self.state.is_empty() {
            return unsafe { std::mem::ManuallyDrop::drop(&mut self.coroutine) };
        }
        if !self.state.can_install() {
            // Forgotten, and so are the registrations its calls hold.
            return self.state.activation.abandon();
        }
        // Safety: dropping the coroutine is what runs while the state is
        // installed, and the coroutine is not used again.
        let _installed = unsafe { self.state.install() };
        unsafe { std::mem::ManuallyDrop::drop(&mut self.coroutine) };
    }
}

impl StorePtrWrapper {
    /// The pointer this wrapper borrows the store through, which stays valid
    /// after the wrapper is dropped for as long as the entry, or the call
    /// through the store's slot, it came from is the innermost one.
    #[inline]
    pub(crate) fn as_ptr(&self) -> *mut StoreInner {
        self.store_ptr
    }

    #[inline]
    pub(crate) fn as_ref(&self) -> StoreRef<'_> {
        // Safety: the store_mut is always initialized unless the StoreMutWrapper
        // is dropped, at which point it's impossible to call this function
        unsafe { self.store_ptr.as_ref().unwrap().as_store_ref() }
    }

    #[inline]
    pub(crate) fn as_mut(&mut self) -> StoreMut<'_> {
        // Safety: the store_mut is always initialized unless the StoreMutWrapper
        // is dropped, at which point it's impossible to call this function
        unsafe { self.store_ptr.as_mut().unwrap().as_store_mut() }
    }
}

impl Clone for StorePtrWrapper {
    fn clone(&self) -> Self {
        if !self.counted {
            return Self { ..*self };
        }
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let top = stack
                .last_mut()
                .expect("No store context installed on this thread");
            match unsafe { top.entry.get().as_ref().unwrap() } {
                StoreContextEntry::Sync(ptr) if *ptr == self.store_ptr => (),
                _ => panic!("Mismatched store context access"),
            }
            top.borrow_count += 1;
            Self {
                store_ptr: self.store_ptr,
                id: self.id,
                counted: true,
            }
        })
    }
}

/// Runs `f` on the active entry while a panic unwinds, if there is one. Never
/// panics: a destructor that panics while unwinding aborts the process.
fn with_active_while_unwinding(f: impl FnOnce(&mut StoreContext)) {
    let _ = STORE_CONTEXT_STACK.try_with(|cell| {
        if let Ok(mut stack) = cell.try_borrow_mut()
            && let Some(top) = stack.last_mut()
        {
            f(top);
        }
    });
}

impl Drop for StorePtrWrapper {
    #[inline]
    fn drop(&mut self) {
        if !self.counted {
            return;
        }
        self.give_back();
    }
}

impl StorePtrWrapper {
    fn give_back(&mut self) {
        if std::thread::panicking() {
            // The borrow is given back while unwinding too, if its entry is
            // still the active one, because a paused borrow is restored then
            // (see `StorePtrPauseGuard`): a host frame further up that catches
            // the panic must find the count of its own borrow. An entry that
            // is no longer active is removed by its install guard, whatever
            // its count.
            with_active_while_unwinding(|top| {
                if unsafe { top.entry.get().as_ref().unwrap() }.as_ptr() == self.store_ptr {
                    top.borrow_count = top.borrow_count.saturating_sub(1);
                }
            });
            return;
        }
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let top = stack
                .last_mut()
                .expect("No store context installed on this thread");
            assert_eq!(top.id, self.id, "Mismatched store context reinstall");
            top.borrow_count -= 1;
        })
    }
}

#[cfg(feature = "experimental-async")]
impl Drop for StoreAsyncGuardWrapper {
    fn drop(&mut self) {
        if std::thread::panicking() {
            // As for `StorePtrWrapper`: the borrow is given back if its entry
            // is still the active one. Only the address of the guard is
            // compared; the entry it points to may be gone.
            with_active_while_unwinding(|top| {
                if let StoreContextEntry::Async { guard, .. } = unsafe { &*top.entry.get() }
                    && std::ptr::eq(guard, self.guard)
                {
                    top.borrow_count = top.borrow_count.saturating_sub(1);
                }
            });
            return;
        }
        let id = self.id;
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let top = stack
                .last_mut()
                .expect("No store context installed on this thread");
            assert_eq!(top.id, id, "Mismatched store context reinstall");
            top.borrow_count -= 1;
        })
    }
}

impl StoreInstallGuard {
    /// "Pause" the borrow a host function holds of the store for as long as
    /// the returned guard lives, which is while Wasm runs: see
    /// [`StoreContext::pause`].
    ///
    /// Only an async context's entry, which the host functions of a call
    /// share, can be borrowed here. An entry this guard pushed starts
    /// unborrowed, and Wasm cannot borrow it before it calls an import, so
    /// there is nothing to pause and nothing is done.
    ///
    /// # Safety
    /// See [`StoreContext::pause`].
    pub(crate) unsafe fn pause(&self) -> Option<StorePtrPauseGuard> {
        match self {
            Self::Pushed(_) => None,
            #[cfg(feature = "experimental-async")]
            Self::Lent { id, .. } => Some(unsafe { StoreContext::pause(*id) }),
        }
    }
}

impl Drop for StoreInstallGuard {
    fn drop(&mut self) {
        let store_id = match *self {
            Self::Pushed(store_id) => store_id,
            #[cfg(feature = "experimental-async")]
            Self::Lent {
                serial, previous, ..
            } => {
                // The entry the call was lent to gets its own pointer back, if
                // it is still there: one made when the call resumed keeps its
                // own. Never panics, also while unwinding.
                let _ = STORE_CONTEXT_STACK.try_with(|cell| {
                    let Ok(mut stack) = cell.try_borrow_mut() else {
                        return;
                    };
                    if let Some(top) = stack.last_mut()
                        && let StoreContextEntry::Async { ptr, serial: s, .. } = top.entry.get_mut()
                        && *s == serial
                    {
                        *ptr = previous;
                    }
                });
                return;
            }
        };
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            match (stack.pop(), std::thread::panicking()) {
                (Some(top), false) => {
                    assert_eq!(top.id, store_id, "Mismatched store context uninstall");
                    assert_eq!(
                        top.borrow_count, 0,
                        "Cannot uninstall store context while it is still borrowed"
                    );
                    top.leave();
                }
                (Some(top), true) => {
                    // If we're panicking and there's a store ID mismatch, just
                    // put the store back in the hope that its own install guard
                    // take care of uninstalling it later.
                    if top.id != store_id {
                        stack.push(top);
                    } else {
                        top.leave();
                    }
                }
                (None, false) => panic!("Store context stack underflow"),
                (None, true) => {
                    // Nothing to do if we're panicking; panics can put the context
                    // in an invalid state, and we don't want to cause another panic here.
                }
            }
        })
    }
}

impl Drop for ForcedStoreInstallGuard {
    fn drop(&mut self) {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            match (stack.pop(), std::thread::panicking()) {
                (Some(top), false) => {
                    assert_eq!(top.id, self.store_id, "Mismatched store context uninstall");
                    assert_eq!(
                        top.borrow_count, 0,
                        "Cannot uninstall store context while it is still borrowed"
                    );
                }
                (Some(top), true) => {
                    // If we're panicking and there's a store ID mismatch, just
                    // put the store back in the hope that its own install guard
                    // take care of uninstalling it later.
                    if top.id != self.store_id {
                        stack.push(top);
                    }
                }
                (None, false) => panic!("Store context stack underflow"),
                (None, true) => {
                    // Nothing to do if we're panicking; panics can put the context
                    // in an invalid state, and we don't want to cause another panic here.
                }
            }
        })
    }
}

impl Drop for StorePtrPauseGuard {
    fn drop(&mut self) {
        if std::thread::panicking() {
            // The paused borrow is restored if its entry is still the active
            // one: the host function that holds it may catch the panic and go
            // on using the store, and its `StorePtrWrapper` gives the borrow
            // back when it is dropped.
            with_active_while_unwinding(|top| {
                if self.ref_count_decremented
                    && top.id == self.store_id
                    && unsafe { top.entry.get().as_ref().unwrap() }.as_ptr() == self.ptr
                {
                    top.borrow_count = top.borrow_count.saturating_add(1);
                }
            });
            return;
        }
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let top = stack
                .last_mut()
                .expect("No store context installed on this thread");
            assert_eq!(top.id, self.store_id, "Mismatched store context access");
            assert_eq!(
                unsafe { top.entry.get().as_ref().unwrap() }.as_ptr(),
                self.ptr,
                "Mismatched store context access"
            );
            if self.ref_count_decremented {
                top.borrow_count += 1;
            }
        })
    }
}

/// Writes through a borrow of a store, as a host function does. Tree Borrows
/// reports the use of a borrow that another one invalidated only where the two
/// conflict, which takes a write, so the tests below write through each borrow
/// they hold rather than only read through it.
#[cfg(test)]
fn touch(store: &mut impl AsStoreMut) {
    let objects = store.objects_mut();
    let id = objects.id();
    match objects {
        #[cfg(feature = "sys")]
        crate::StoreObjects::Sys(objects) => objects.set_id(id),
        #[cfg(feature = "v8")]
        crate::StoreObjects::V8(objects) => objects.set_id(id),
        #[cfg(feature = "js")]
        crate::StoreObjects::Js(objects) => objects.set_id(id),
    }
}

#[cfg(test)]
mod borrow_provenance {
    use super::*;
    use crate::{AsStoreMut, Store};

    /// Replays a recursive guest call at the level of the store context, which
    /// is the part of it that has nothing to do with executing Wasm:
    ///
    /// ```text
    /// Function::call(&mut store)          install(ptr from the caller's borrow)
    ///   wasm -> import                    get_current            -> the shim's borrow
    ///     shim: Function::call(&mut env)  install(ptr from *that* borrow)
    ///       wasm -> import                get_current            -> the inner borrow
    ///     shim carries on using its own borrow
    /// ```
    ///
    /// Natively this always passes; it earns its keep under Miri, which is
    /// where the last step is checked. If [`StoreContext::install`] ever goes
    /// back to re-using an entry that is already active, the inner borrow
    /// becomes a sibling of the shim's rather than a child of it, and this test
    /// reports the shim's own store as invalidated:
    ///
    /// ```text
    /// error: Undefined Behavior: trying to retag from <..> for Unique
    ///        permission, but that tag does not exist in the borrow stack
    ///   --> lib/api/src/entities/store/store_ref.rs   &mut self.inner.objects
    /// ```
    ///
    /// Run it with:
    ///
    /// ```text
    /// cargo +nightly miri test -p wasmer --features sys --lib borrow_provenance
    /// ```
    #[test]
    fn nested_call_keeps_the_outer_borrow_usable() {
        let mut store = Store::default();
        let id = store.id();

        // --- the embedder's Function::call(&mut store)
        let mut caller = store.as_store_mut();
        let install = unsafe { StoreContext::install(caller.as_store_mut().inner as *mut _) };

        // --- the import trampoline, handing the shim its store
        let mut wrapper = unsafe { StoreContext::get_current(id) };
        let mut shim = wrapper.as_mut();
        touch(&mut shim);

        {
            // --- the shim calls back into the guest
            let inner_install =
                unsafe { StoreContext::install(shim.as_store_mut().inner as *mut _) };
            let pause = unsafe { inner_install.pause() };
            assert!(pause.is_none(), "a pushed entry has no borrow to pause");

            // --- the inner import trampoline
            let mut inner_wrapper = unsafe { StoreContext::get_current(id) };
            let mut inner_shim = inner_wrapper.as_mut();
            touch(&mut inner_shim);
            drop(inner_wrapper);

            drop(pause);
            drop(inner_install);
        }

        // --- and goes on using the borrow it held throughout
        touch(&mut shim);

        drop(wrapper);
        drop(install);
    }

    /// An import trampoline finishes on the Wasm stack, after its host
    /// function gave the borrow back, with the pointer the borrow was taken
    /// through; then the caller goes on with its own borrow. Miri checks both.
    #[test]
    fn an_import_finishes_with_the_pointer_it_borrowed_through() {
        let mut store = Store::default();
        let id = store.id();

        let mut caller = store.as_store_mut();
        let install = unsafe { StoreContext::install(caller.as_store_mut().inner as *mut _) };

        // --- the import trampoline, on the host stack
        let store_ptr = {
            let mut wrapper = unsafe { StoreContext::get_current(id) };
            let store_ptr = wrapper.as_ptr();
            touch(&mut wrapper.as_mut());
            store_ptr
        };
        // --- back on the Wasm stack
        touch(unsafe { store_ptr.as_mut().unwrap() });

        drop(install);
        touch(&mut caller);
    }

    /// The lending path, which is the nesting above with the inner entry
    /// installed by [`StoreMut::parked`] instead of by a nested call: the
    /// borrow handed out by [`crate::Store::with_current`] must not invalidate
    /// the one the lender is holding. Miri checks the last line.
    #[test]
    fn a_lend_keeps_the_lending_borrow_usable() {
        let mut store = Store::default();
        let id = store.id();

        let mut caller = store.as_store_mut();
        let install = unsafe { StoreContext::install(caller.as_store_mut().inner as *mut _) };

        let mut wrapper = unsafe { StoreContext::get_current(id) };
        let mut shim = wrapper.as_mut();
        touch(&mut shim);

        shim.parked(|| {
            crate::Store::with_current(|lent| {
                touch(lent);
            })
            .expect("a parked store is lendable");
        });

        // The lender goes back to the borrow it held throughout.
        touch(&mut shim);

        drop(wrapper);
        drop(install);
    }

    /// A host function that catches a panic from the guest it called goes on
    /// with the borrow it held throughout, after the nested entry, the pause
    /// and the borrow of the host function that panicked were dropped while
    /// unwinding. Miri checks the last use.
    #[test]
    fn a_caught_panic_keeps_the_outer_borrow_usable() {
        let mut store = Store::default();
        let id = store.id();

        let mut caller = store.as_store_mut();
        let install = unsafe { StoreContext::install(caller.as_store_mut().inner as *mut _) };

        let mut wrapper = unsafe { StoreContext::get_current(id) };
        let mut shim = wrapper.as_mut();
        touch(&mut shim);

        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let inner_install =
                unsafe { StoreContext::install(shim.as_store_mut().inner as *mut _) };
            let _pause = unsafe { inner_install.pause() };
            let mut inner_wrapper = unsafe { StoreContext::get_current(id) };
            touch(&mut inner_wrapper.as_mut());
            panic!("host function panicked");
        }));
        assert!(caught.is_err());

        touch(&mut shim);

        drop(wrapper);
        drop(install);
    }
}

/// The tests of [`borrow_provenance`], for a plain store, whose calls go
/// through its slot (see [`SlotRef`]) rather than the context stack. Each
/// replays what `Function::call` ([`StoreContext::enter`]) and an import
/// trampoline ([`StoreContext::get_current_in`]) do, in the order they do it,
/// and writes through every borrow it holds, so that both Stacked and Tree
/// Borrows report a nested call that does not write its own pointer:
///
/// ```text
/// cargo +nightly miri test -p wasmer --lib --no-default-features \
///     --features sys,headless,experimental-async -- borrow_provenance
/// ```
#[cfg(all(test, feature = "sys"))]
mod borrow_provenance_slot {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;
    use crate::{AsStoreMut, Function, Store};

    fn slot_of(store: &Store) -> SlotRef {
        SlotRef::of(&store.inner.objects)
    }

    fn enter(store: &mut StoreMut<'_>) -> StoreEntryGuard {
        let guard = unsafe { StoreContext::enter(store.as_store_mut().inner as *mut _) };
        assert!(
            matches!(guard, StoreEntryGuard::Slot { .. }),
            "a plain store is entered through its slot"
        );
        guard
    }

    /// A call through the slot leaves the calling thread there with its
    /// store, for the host functions it reaches to take, and puts back what
    /// was there before when it returns.
    #[test]
    fn a_call_through_the_slot_leaves_its_thread_there() {
        let mut store = Store::default();
        let slot = slot_of(&store);
        assert!(slot.thread().is_none());
        let this_thread = wasmer_vm::CurrentThread::get();
        let mut caller = store.as_store_mut();
        let entry = enter(&mut caller);
        let thread = slot.thread().expect("the call left its thread");
        assert!(std::ptr::eq(thread.slot_call(), this_thread.slot_call()));
        drop(entry);
        assert!(slot.thread().is_none());
        assert!(slot.get().is_none());
    }

    /// `Function::call(&mut store)` -> import -> the host function calls the
    /// guest again with its own borrow -> import; then the host function goes
    /// on with the borrow it held throughout. If the nested call did not write
    /// its own pointer into the slot, the inner import's borrow would be a
    /// sibling of the host function's, and Miri would report its last use.
    #[test]
    fn nested_call_keeps_the_outer_borrow_usable() {
        let mut store = Store::default();
        let id = store.id();
        let slot = slot_of(&store);

        let mut caller = store.as_store_mut();
        let entry = enter(&mut caller);
        assert!(!StoreContext::is_empty(), "a call is running");
        assert!(unsafe { entry.pause() }.is_none(), "nothing to pause");

        // --- the import trampoline, handing the host function its store
        let mut wrapper = unsafe { StoreContext::get_current_in(slot, id) };
        let mut shim = wrapper.as_mut();
        touch(&mut shim);

        {
            // --- the host function calls back into the guest
            let inner_entry = enter(&mut shim);

            // --- the inner import trampoline
            let mut inner_wrapper = unsafe { StoreContext::get_current_in(slot, id) };
            touch(&mut inner_wrapper.as_mut());
            drop(inner_wrapper);

            drop(inner_entry);
        }

        // --- and goes on using the borrow it held throughout
        touch(&mut shim);
        assert_eq!(
            slot.get().map(NonNull::as_ptr),
            Some(wrapper.as_ptr()),
            "the nested call put the outer pointer back"
        );

        drop(wrapper);
        drop(entry);
        assert!(slot.get().is_none(), "no call is running");
        assert!(StoreContext::is_empty());
        touch(&mut caller);
    }

    /// An import trampoline finishes on the Wasm stack with the pointer it
    /// borrowed the store through; then the caller goes on with its own
    /// borrow.
    #[test]
    fn an_import_finishes_with_the_pointer_it_borrowed_through() {
        let mut store = Store::default();
        let id = store.id();
        let slot = slot_of(&store);

        let mut caller = store.as_store_mut();
        let entry = enter(&mut caller);

        // --- the import trampoline, on the host stack
        let store_ptr = {
            let mut wrapper = unsafe { StoreContext::get_current_in(slot, id) };
            let store_ptr = wrapper.as_ptr();
            touch(&mut wrapper.as_mut());
            store_ptr
        };
        // --- back on the Wasm stack
        touch(unsafe { store_ptr.as_mut().unwrap() });
        assert_eq!(
            unsafe { StoreContext::get_current_transient_in(slot, id) },
            store_ptr
        );

        drop(entry);
        touch(&mut caller);
    }

    /// Lending inside a call through the slot: the store is not lendable
    /// while the host function holds it, even with an outer park on the
    /// stack; parked, it is, and a call made with the lent store nests under
    /// the lender's borrow.
    #[test]
    fn a_lend_keeps_the_lending_borrow_usable() {
        let mut other = Store::default();
        let mut store = Store::default();
        let id = store.id();
        let slot = slot_of(&store);

        // An outer park of another store, which a call into this one hides.
        other.as_store_mut().parked(|| {
            assert!(crate::Store::with_current(|_| ()).is_some());

            let mut caller = store.as_store_mut();
            let entry = enter(&mut caller);

            let mut wrapper = unsafe { StoreContext::get_current_in(slot, id) };
            let mut shim = wrapper.as_mut();
            touch(&mut shim);
            assert!(
                crate::Store::with_current(|_| ()).is_none(),
                "the host function holds its store; the outer park is hidden"
            );

            shim.parked(|| {
                crate::Store::with_current(|lent| {
                    assert_eq!(lent.objects_mut().id(), id, "the parked store is lent");
                    // --- the lent store calls into the guest
                    let inner_entry = enter(lent);
                    let mut inner_wrapper = unsafe { StoreContext::get_current_in(slot, id) };
                    touch(&mut inner_wrapper.as_mut());
                    assert!(crate::Store::with_current(|_| ()).is_none());
                    drop(inner_wrapper);
                    drop(inner_entry);
                    touch(lent);
                })
                .expect("a parked store is lendable");
            });

            // The lender goes back to the borrow it held throughout.
            touch(&mut shim);
            assert!(crate::Store::with_current(|_| ()).is_none());

            drop(wrapper);
            drop(entry);
            touch(&mut caller);
            assert!(
                crate::Store::with_current(|_| ()).is_some(),
                "the outer park is visible again"
            );
        });
    }

    /// A host function that catches a panic from the guest it called goes on
    /// with the borrow it held throughout: the nested call's guard, dropped
    /// while unwinding, put the slot back.
    #[test]
    fn a_caught_panic_keeps_the_outer_borrow_usable() {
        let mut store = Store::default();
        let id = store.id();
        let slot = slot_of(&store);

        let mut caller = store.as_store_mut();
        let entry = enter(&mut caller);

        let mut wrapper = unsafe { StoreContext::get_current_in(slot, id) };
        let mut shim = wrapper.as_mut();
        touch(&mut shim);

        let caught = catch_unwind(AssertUnwindSafe(|| {
            let _inner_entry = enter(&mut shim);
            let mut inner_wrapper = unsafe { StoreContext::get_current_in(slot, id) };
            touch(&mut inner_wrapper.as_mut());
            panic!("host function panicked");
        }));
        assert!(caught.is_err());

        touch(&mut shim);
        assert_eq!(slot.get().map(NonNull::as_ptr), Some(wrapper.as_ptr()));
        assert!(crate::Store::with_current(|_| ()).is_none());

        drop(wrapper);
        drop(entry);
        assert!(slot.get().is_none());
        assert!(StoreContext::is_empty());
    }

    /// A host function of one store calls into another: each store's slot
    /// holds its own call's pointer, and the first host function's borrow
    /// stays usable.
    #[test]
    fn a_second_store_entered_from_an_import() {
        let mut first = Store::default();
        let mut second = Store::default();
        let (first_id, second_id) = (first.id(), second.id());
        let (first_slot, second_slot) = (slot_of(&first), slot_of(&second));

        let mut caller = first.as_store_mut();
        let entry = enter(&mut caller);
        let mut wrapper = unsafe { StoreContext::get_current_in(first_slot, first_id) };
        let mut shim = wrapper.as_mut();
        touch(&mut shim);

        {
            // --- the host function calls into the second store
            let mut second_caller = second.as_store_mut();
            let second_entry = enter(&mut second_caller);
            let mut second_wrapper =
                unsafe { StoreContext::get_current_in(second_slot, second_id) };
            touch(&mut second_wrapper.as_mut());
            // --- whose host function calls back into the first, nested
            let inner_entry = enter(&mut shim);
            let mut inner_wrapper = unsafe { StoreContext::get_current_in(first_slot, first_id) };
            touch(&mut inner_wrapper.as_mut());
            drop(inner_wrapper);
            drop(inner_entry);
            touch(&mut second_wrapper.as_mut());
            drop(second_wrapper);
            drop(second_entry);
            assert!(second_slot.get().is_none());
            touch(&mut second_caller);
        }

        touch(&mut shim);
        drop(wrapper);
        drop(entry);
        assert!(first_slot.get().is_none());
        touch(&mut caller);
    }

    /// A store's slot lives and dies with it: host functions keep a pointer
    /// to it, which a store made later, possibly at the same address, does
    /// not share.
    #[test]
    fn a_store_made_again_has_a_slot_of_its_own() {
        let mut store = Store::default();
        let _host = Function::new_typed(&mut store, || {});
        {
            let mut caller = store.as_store_mut();
            let _entry = enter(&mut caller);
        }
        drop(store);

        for _ in 0..4 {
            let mut store = Store::default();
            let id = store.id();
            let slot = slot_of(&store);
            // The old store's slot was freed with it, after its host
            // function; Miri reports a use of it, or a leak.
            let _host = Function::new_typed(&mut store, || {});
            let mut caller = store.as_store_mut();
            let entry = enter(&mut caller);
            let mut wrapper = unsafe { StoreContext::get_current_in(slot, id) };
            touch(&mut wrapper.as_mut());
            drop(wrapper);
            drop(entry);
            assert!(slot.get().is_none());
        }
    }

    /// The objects of a store, and the host functions among them, can be
    /// moved to another store; their slot goes with them, so that a call into
    /// the other store writes the slot its host functions read, and the store
    /// they were made in can be dropped.
    #[test]
    fn objects_moved_to_another_store_take_their_slot() {
        let mut first = Store::default();
        let mut second = Store::default();
        let _host = Function::new_typed(&mut first, || {});
        let id = first.id();
        let slot = slot_of(&first);
        std::mem::swap(first.objects_mut(), second.objects_mut());
        assert_eq!(slot_of(&second).0, slot.0);
        drop(first);

        let mut caller = second.as_store_mut();
        let entry = enter(&mut caller);
        let mut wrapper = unsafe { StoreContext::get_current_in(slot, id) };
        assert_eq!(wrapper.as_ptr(), caller.inner as *mut StoreInner);
        touch(&mut wrapper.as_mut());
        drop(wrapper);
        drop(entry);
        assert!(slot.get().is_none());
        touch(&mut caller);
    }

    /// `Module::instantiate` builds the instance through a borrow of the
    /// store's objects, then runs the start function, whose imports borrow
    /// the store through the pointer the call entered with, and on a trap
    /// borrows the objects again to keep the instance. The pointer is taken
    /// after the first borrow of the objects ends: taken before it, as it once
    /// was, it is invalidated by that borrow, and Miri reports the import.
    #[test]
    fn instantiation_enters_after_it_built_the_instance() {
        let mut store = Store::default();
        let id = store.id();
        let slot = slot_of(&store);
        let mut store_mut = store.as_store_mut();

        // --- the instance is built
        let (_engine, objects) = store_mut.engine_and_objects_mut();
        let id_of_objects = objects.id();
        objects.as_sys_mut().set_id(id_of_objects);

        // --- the start function runs
        let store_ptr = store_mut.inner as *mut StoreInner;
        let entry = unsafe { StoreContext::enter(store_ptr) };
        let mut wrapper = unsafe { StoreContext::get_current_in(slot, id) };
        touch(&mut wrapper.as_mut());
        drop(wrapper);

        // --- and traps: the instance is kept
        drop(entry);
        touch(&mut store_mut);
    }

    /// A store used from coroutines keeps to the context stack, and its host
    /// functions find it there.
    #[cfg(feature = "experimental-async")]
    #[test]
    fn an_async_store_is_entered_through_the_stack() {
        let store = Store::default().into_async();
        let id = store.id;
        let mut write = store.inner.try_write().unwrap();
        let slot = SlotRef::of(&write.objects);
        let entry = unsafe { StoreContext::enter(&raw mut **write) };
        assert!(matches!(entry, StoreEntryGuard::Stack(_)));
        assert!(slot.get().is_none());
        let mut wrapper = unsafe { StoreContext::get_current_in(slot, id) };
        assert!(wrapper.counted);
        touch(&mut wrapper.as_mut());
        drop(wrapper);
        drop(entry);
        drop(write);
        let mut store = store.into_store().unwrap();
        let mut caller = store.as_store_mut();
        drop(enter(&mut caller));
    }
}

#[cfg(all(test, feature = "experimental-async"))]
mod unwinding {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;
    use crate::Store;

    fn active_borrows() -> u32 {
        STORE_CONTEXT_STACK.with(|cell| cell.borrow().last().unwrap().borrow_count)
    }

    /// An async context has one entry for all the host functions of a call:
    /// a guest call pauses the borrow of the host function that makes it,
    /// rather than installing an entry of its own. When a host function the
    /// guest reaches panics, and the one that made the guest call catches the
    /// panic, the paused borrow is restored and the borrow of the one that
    /// panicked given back while unwinding, so that the count is that of the
    /// borrow still held.
    #[test]
    fn a_caught_panic_leaves_the_borrow_of_the_host_function_that_caught_it() {
        let store = Store::default().into_async();
        let id = store.id;
        let install = StoreContext::install_async(store.inner.try_write().unwrap());

        // --- the host function that catches the panic
        let mut wrapper = unsafe { StoreContext::get_current(id) };
        assert_eq!(active_borrows(), 1);
        let caught = catch_unwind(AssertUnwindSafe(|| {
            // --- its guest call
            let guest_call = unsafe { StoreContext::install(wrapper.as_mut().inner as *mut _) };
            assert!(matches!(guest_call, StoreInstallGuard::Lent { .. }));
            let _pause = unsafe { guest_call.pause() };
            assert_eq!(active_borrows(), 0);
            // --- a host function the guest reaches, which panics
            let _inner_wrapper = unsafe { StoreContext::get_current(id) };
            assert_eq!(active_borrows(), 1);
            panic!("host function panicked");
        }));
        assert!(caught.is_err());
        assert_eq!(active_borrows(), 1);

        let _ = wrapper.as_mut().objects_mut().id();
        drop(wrapper);
        drop(install);
        assert!(StoreContext::is_empty());
        assert!(store.into_store().is_ok());
    }

    /// A synchronous host function of a call in an async context calls its
    /// guest back, and goes on with its store once that returns, as one of
    /// a synchronous call does (see `borrow_provenance`): the borrows of the
    /// host functions that guest call reaches are children of its own. Miri
    /// checks the last write.
    #[test]
    fn a_nested_call_keeps_the_outer_borrow_usable() {
        // Writes the store's objects back as they are: a write through
        // `store`, which Tree Borrows needs to see a sibling's.
        fn write(store: &mut StoreMut<'_>) {
            let objects = store.objects_mut();
            unsafe { std::ptr::write(objects, std::ptr::read(objects)) };
        }

        let store = Store::default().into_async();
        let id = store.id;
        let install = StoreContext::install_async(store.inner.try_write().unwrap());

        // --- the host function, handed its store
        let mut wrapper = unsafe { StoreContext::get_current(id) };
        let mut outer = wrapper.as_mut();
        write(&mut outer);
        {
            // --- its guest call
            let _install = unsafe { StoreContext::install(outer.as_store_mut().inner as *mut _) };
            let _pause = unsafe { StoreContext::pause(id) };
            // --- a host function that guest call reaches
            let mut inner = unsafe { StoreContext::get_current(id) };
            write(&mut inner.as_mut());
        }
        // --- the host function goes on with its store
        write(&mut outer);

        drop(wrapper);
        drop(install);
        assert!(store.into_store().is_ok());
    }

    /// A guest call two host functions deep suspends: the entry is taken off,
    /// the store is used by another task, and the call resumes with a new
    /// entry. When the inner guest call returns, the new entry keeps its own
    /// pointer, rather than getting back the one the outer guest call lent,
    /// which the other task's use invalidated. Miri checks, under Stacked
    /// Borrows, the write of a host function its guest calls next.
    #[test]
    fn a_call_that_suspended_leaves_the_new_entry_its_pointer() {
        fn write(store: &mut StoreMut<'_>) {
            let objects = store.objects_mut();
            unsafe { std::ptr::write(objects, std::ptr::read(objects)) };
        }
        let store = Store::default().into_async();
        let id = store.id;
        let install = StoreContext::install_async(store.inner.try_write().unwrap());

        // --- a host function, and its guest call
        let mut outer = unsafe { StoreContext::get_current(id) };
        let outer_call = unsafe { StoreContext::install(outer.as_mut().inner as *mut _) };
        let outer_pause = unsafe { StoreContext::pause(id) };
        // --- a host function of that guest call, and its own guest call
        let mut inner = unsafe { StoreContext::get_current(id) };
        let inner_call = unsafe { StoreContext::install(inner.as_mut().inner as *mut _) };
        let inner_pause = unsafe { StoreContext::pause(id) };
        // --- which suspends; another task uses the store, and it resumes
        drop(install);
        {
            let mut other = store.inner.try_write().unwrap();
            write(&mut StoreMut {
                inner: &mut **other,
            });
        }
        let install = StoreContext::install_async(store.inner.try_write().unwrap());
        // --- the inner guest call returns, and so does its host function
        drop(inner_pause);
        drop(inner_call);
        drop(inner);
        // --- the outer guest call goes on, with another host function
        let mut next = unsafe { StoreContext::get_current(id) };
        write(&mut next.as_mut());
        drop(next);
        drop(outer_pause);
        drop(outer_call);
        drop(outer);

        drop(install);
        assert!(store.into_store().is_ok());
    }

    /// A panic whose host function did not make the guest call, so that
    /// nothing was paused, leaves the count where it was.
    #[test]
    fn a_caught_panic_without_a_guest_call_leaves_the_count_alone() {
        let store = Store::default().into_async();
        let id = store.id;
        let install = StoreContext::install_async(store.inner.try_write().unwrap());

        let wrapper = unsafe { StoreContext::get_current(id) };
        let caught = catch_unwind(AssertUnwindSafe(|| {
            let _inner_wrapper = unsafe { StoreContext::get_current(id) };
            assert_eq!(active_borrows(), 2);
            panic!("host function panicked");
        }));
        assert!(caught.is_err());
        assert_eq!(active_borrows(), 1);

        drop(wrapper);
        drop(install);
        assert!(store.into_store().is_ok());
    }

    /// The same for the borrow an async host function takes while it reads
    /// its arguments.
    #[test]
    fn a_caught_panic_gives_back_the_borrow_of_an_async_host_function() {
        let store = Store::default().into_async();
        let id = store.id;
        let install = StoreContext::install_async(store.inner.try_write().unwrap());

        let wrapper = unsafe { StoreContext::get_current(id) };
        let caught = catch_unwind(AssertUnwindSafe(|| {
            let inner = unsafe { StoreContext::try_get_current_async(id) };
            assert!(matches!(inner, GetStoreAsyncGuardResult::Ok(_)));
            assert_eq!(active_borrows(), 2);
            panic!("async host function panicked");
        }));
        assert!(caught.is_err());
        assert_eq!(active_borrows(), 1);

        drop(wrapper);
        drop(install);
        assert!(store.into_store().is_ok());
    }
}

/// What [`Cothread`] does with the store context entries of a coroutine,
/// replayed without Wasm, so that Miri checks it: a "suspended call" is an
/// install guard kept across the end of a resume, as the frames of a call
/// are kept on the stack of a suspended coroutine.
#[cfg(all(test, feature = "unsafe-cothread"))]
mod cothread {
    use std::{cell::RefCell, rc::Rc};

    use super::*;
    use crate::{AsStoreMut, Store};

    type Call = Rc<RefCell<Option<(StoreInstallGuard, Option<StorePtrWrapper>)>>>;

    /// The one pointer to the store that every borrow below derives from, as
    /// the frames of a call derive theirs from the borrow the call was given.
    /// The store must stay where it is while the pointer is used.
    fn pointer(store: &mut Store) -> (StoreId, *mut StoreInner) {
        (store.id(), store.as_store_mut().inner)
    }

    /// The coroutine enters the store, and its host function, holding a
    /// borrow of the store, suspends it.
    fn enter_and_suspend(ptr: *mut StoreInner, call: &Call) {
        let id = unsafe { (*ptr).objects.id() };
        let install = unsafe { StoreContext::install(ptr) };
        let mut wrapper = unsafe { StoreContext::get_current(id) };
        let _ = wrapper.as_mut().objects_mut().id();
        *RefCell::borrow_mut(call) = Some((install, Some(wrapper)));
    }

    /// The host function is resumed, uses its borrow, and the call returns.
    fn resume_and_return(id: StoreId, call: &Call) {
        assert!(StoreContext::is_active(id), "the call finds its entry");
        let (install, wrapper) = RefCell::borrow_mut(call).take().unwrap();
        let mut wrapper = wrapper.unwrap();
        let _ = wrapper.as_mut().objects_mut().id();
        drop(wrapper);
        drop(install);
    }

    #[test]
    fn a_suspended_call_takes_its_entry_with_it() {
        let mut store = Store::default();
        let (id, ptr) = pointer(&mut store);
        let call = Call::default();
        // Safety: no coroutine; the tests replay its state by hand.
        let mut cothread = unsafe { Cothread::new(()) };

        unsafe { cothread.resume(|_| enter_and_suspend(ptr, &call)) };
        assert!(StoreContext::is_empty(), "the entry is off the thread");
        assert!(!cothread.state.is_empty());

        unsafe { cothread.resume(|_| resume_and_return(id, &call)) };
        assert!(StoreContext::is_empty());
        assert!(
            cothread.state.is_empty(),
            "nothing is kept once the call returned"
        );
    }

    #[test]
    fn a_cothread_is_resumed_above_the_calls_of_the_thread() {
        let (mut a, mut b) = (Store::default(), Store::default());
        let ((a_id, a_ptr), (b_id, b_ptr)) = (pointer(&mut a), pointer(&mut b));
        let call = Call::default();
        // Safety: no coroutine; the tests replay its state by hand.
        let mut cothread = unsafe { Cothread::new(()) };
        unsafe { cothread.resume(|_| enter_and_suspend(a_ptr, &call)) };

        // Resumed from a host function of a call on another store.
        let outer = unsafe { StoreContext::install(b_ptr) };
        unsafe { cothread.resume(|_| resume_and_return(a_id, &call)) };
        assert!(
            StoreContext::is_active(b_id),
            "the outer call's entry is untouched"
        );
        drop(outer);
        assert!(StoreContext::is_empty());
    }

    /// Unwinds the call when dropped, as dropping a suspended coroutine
    /// unwinds the frames on its stack.
    struct Unwinds(StoreId, Call);
    impl Drop for Unwinds {
        fn drop(&mut self) {
            resume_and_return(self.0, &self.1);
        }
    }

    #[test]
    fn dropping_a_cothread_unwinds_it_with_its_entries() {
        let mut store = Store::default();
        let (id, ptr) = pointer(&mut store);
        let call = Call::default();
        // Safety: `Unwinds` stands for an owned coroutine: dropping it is
        // what unwinding one does.
        let mut cothread = unsafe { Cothread::new(Unwinds(id, call.clone())) };
        unsafe { cothread.resume(|_| enter_and_suspend(ptr, &call)) };
        assert!(StoreContext::is_empty());
        drop(cothread);
        assert!(call.borrow().is_none(), "the call was unwound");
        assert!(StoreContext::is_empty());
    }

    #[test]
    fn a_cothread_is_not_resumed_while_its_store_is_used_on_the_thread() {
        let mut store = Store::default();
        let (id, ptr) = pointer(&mut store);
        let call = Call::default();
        // Safety: no coroutine; the tests replay its state by hand.
        let mut cothread = unsafe { Cothread::new(()) };
        unsafe { cothread.resume(|_| enter_and_suspend(ptr, &call)) };

        // Another call on the store, made on the thread meanwhile.
        let other_call = unsafe { StoreContext::install(ptr) };
        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            cothread.resume(|_| ());
        }));
        assert!(refused.is_err(), "the store is already on the thread");
        drop(other_call);
        assert!(StoreContext::is_empty());

        // The suspended call is untouched, and can be resumed now.
        unsafe { cothread.resume(|_| resume_and_return(id, &call)) };
        assert!(cothread.state.is_empty());
    }

    type EnteredCall = Rc<RefCell<Option<(StoreEntryGuard, StorePtrWrapper)>>>;

    /// What [`enter_and_suspend`] does, entering the store as a call does:
    /// [`StoreContext::enter`], and the host function's lookup through the
    /// slot.
    fn enter_through_the_slot_and_suspend(ptr: *mut StoreInner, call: &EnteredCall) {
        let (id, slot) = unsafe { ((*ptr).objects.id(), SlotRef::of(&(*ptr).objects)) };
        let entry = unsafe { StoreContext::enter(ptr) };
        let mut wrapper = unsafe { StoreContext::get_current_in(slot, id) };
        let _ = wrapper.as_mut().objects_mut().id();
        *RefCell::borrow_mut(call) = Some((entry, wrapper));
    }

    /// What [`resume_and_return`] does, for a call made by
    /// [`enter_through_the_slot_and_suspend`].
    fn resume_entered_and_return(call: &EnteredCall) {
        let (entry, mut wrapper) = RefCell::borrow_mut(call).take().unwrap();
        let _ = wrapper.as_mut().objects_mut().id();
        drop(wrapper);
        drop(entry);
    }

    /// A coroutine's call into a plain store goes through the context stack,
    /// which the `Cothread` takes with it, rather than through the slot, and
    /// its host functions find it there.
    #[test]
    fn a_coroutine_enters_a_plain_store_through_the_stack() {
        let mut store = Store::default();
        let (id, ptr) = pointer(&mut store);
        let slot = SlotRef::of(&store.inner.objects);
        let call = EnteredCall::default();
        // Safety: no coroutine; the tests replay its state by hand.
        let mut cothread = unsafe { Cothread::new(()) };
        unsafe {
            cothread.resume(|_| {
                enter_through_the_slot_and_suspend(ptr, &call);
                assert!(StoreContext::is_active(id), "the call has an entry");
                assert!(slot.get().is_none(), "and the slot is empty");
                let borrowed = call.borrow();
                let (entry, wrapper) = borrowed.as_ref().unwrap();
                assert!(matches!(entry, StoreEntryGuard::Stack(_)));
                assert!(wrapper.counted, "the host function borrows the entry");
            })
        };
        assert!(StoreContext::is_empty(), "the entry is off the thread");
        assert!(!wasmer_vm::CurrentThread::get().cothread().get());

        // Outside the coroutine, calls go through the slot again.
        let entry = unsafe { StoreContext::enter(ptr) };
        assert!(matches!(entry, StoreEntryGuard::Slot { .. }));
        drop(entry);

        unsafe { cothread.resume(|_| resume_entered_and_return(&call)) };
        assert!(StoreContext::is_empty());
        assert!(cothread.state.is_empty());
    }

    /// A coroutine resumed from a host function of a call into a plain store
    /// may be lent the store and call into it: its call hides the slot from
    /// its host functions, and the slot holds the call beneath again when the
    /// coroutine suspends, and once its call returns.
    #[test]
    fn a_coroutine_lent_a_plain_store_hides_its_slot() {
        let mut store = Store::default();
        let (id, ptr) = pointer(&mut store);
        let slot = SlotRef::of(&store.inner.objects);
        let call = EnteredCall::default();
        // Safety: no coroutine; the tests replay its state by hand.
        let mut cothread = unsafe { Cothread::new(()) };

        // The call beneath, and its host function.
        let outer = unsafe { StoreContext::enter(ptr) };
        assert!(matches!(outer, StoreEntryGuard::Slot { .. }));
        let mut host = unsafe { StoreContext::get_current_in(slot, id) };
        assert!(!host.counted, "the host function borrows through the slot");
        // Lent to the coroutine, through the pointer of the call beneath.
        let lent = ptr;

        unsafe {
            cothread.resume(|_| {
                enter_through_the_slot_and_suspend(lent, &call);
                assert!(slot.get().is_none(), "the coroutine's call hides the slot");
            })
        };
        assert_eq!(
            slot.get().map(NonNull::as_ptr),
            Some(lent),
            "the slot is back"
        );
        let _ = host.as_mut().objects_mut().id();

        // While the call beneath holds the store, the coroutine cannot go on.
        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            cothread.resume(|_| ());
        }));
        assert!(refused.is_err(), "the store is in use through its slot");
        assert!(!cothread.state.can_install());
        drop(host);
        drop(outer);
        assert!(slot.get().is_none());

        unsafe {
            cothread.resume(|_| {
                assert!(StoreContext::is_active(id));
                resume_entered_and_return(&call);
                assert!(slot.get().is_none());
            })
        };
        assert!(StoreContext::is_empty());
        assert!(cothread.state.is_empty());
    }
}
