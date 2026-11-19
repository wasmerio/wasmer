//! Thread-local storage for storing the current store context,
//! i.e. the currently active [`Store`](crate::Store)(s). When
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
//! [`StoreAsync`](crate::StoreAsync); This achieves two goals:
//!   * Makes the [`StoreAsync`](crate::StoreAsync) available
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

use std::{
    borrow::BorrowMut,
    cell::{RefCell, UnsafeCell},
    mem::MaybeUninit,
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
        /// from a shared reference it would then write through.
        ptr: *mut StoreInner,
    },
}

impl StoreContextEntry {
    #[cfg(feature = "experimental-async")]
    fn new_async(mut guard: LocalRwLockWriteGuard<Box<StoreInner>>) -> Self {
        // A raw pointer to the box's contents, not a reference to them: it
        // stays valid when something else borrows the store through the lock,
        // as a future does while the store is lent out, so a borrow paused
        // across that can be picked up again.
        let ptr: *mut StoreInner = &raw mut **guard;
        Self::Async { guard, ptr }
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
}

pub(crate) struct StorePtrWrapper {
    store_ptr: *mut StoreInner,
}

#[cfg(feature = "experimental-async")]
pub(crate) struct StoreAsyncGuardWrapper {
    pub(crate) guard: *mut LocalRwLockWriteGuard<Box<StoreInner>>,
    /// The entry's pointer to the store, to borrow it through rather than
    /// through `guard`.
    pub(crate) store_ptr: *mut StoreInner,
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

pub(crate) struct StoreInstallGuard {
    /// `None` when this guard installed nothing, which happens only for a
    /// store an async context already holds — see [`StoreContext::install`].
    store_id: Option<StoreId>,
}

thread_local! {
    static STORE_CONTEXT_STACK: RefCell<Vec<StoreContext>> = const { RefCell::new(Vec::new()) };
}

impl StoreContext {
    /// Whether `id`'s entry is the active one.
    pub(crate) fn is_active(id: StoreId) -> bool {
        STORE_CONTEXT_STACK.with(|cell| {
            let stack = cell.borrow();
            stack.last().is_some_and(|ctx| ctx.id == id)
        })
    }

    fn is_suspended(id: StoreId) -> bool {
        !Self::is_active(id)
            && STORE_CONTEXT_STACK.with(|cell| {
                let stack = cell.borrow();
                stack.iter().rev().skip(1).any(|ctx| ctx.id == id)
            })
    }

    fn push(id: StoreId, entry: StoreContextEntry) {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            stack.push(Self {
                id,
                borrow_count: 0,
                entry: UnsafeCell::new(entry),
            });
        })
    }

    /// Returns true if there are no active store context entries.
    pub(crate) fn is_empty() -> bool {
        STORE_CONTEXT_STACK.with(|cell| {
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

    /// Whether the active entry is `id`'s, held by an async context.
    fn active_is_async(id: StoreId) -> bool {
        STORE_CONTEXT_STACK.with(|cell| {
            let stack = cell.borrow();
            let Some(top) = stack.last() else {
                return false;
            };
            if top.id != id {
                return false;
            }
            match unsafe { top.entry.get().as_ref().unwrap() } {
                StoreContextEntry::Sync(_) => false,
                #[cfg(feature = "experimental-async")]
                StoreContextEntry::Async { .. } => true,
            }
        })
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
    /// An async context is the exception, and installs nothing: it owns the
    /// store through a write guard held in the entry itself, and every
    /// re-acquisition re-derives from that guard rather than from a caller's
    /// borrow (see `AsyncCallStoreMut`), so there is no borrow to nest under.
    /// Shadowing it would also hide the guard from async host functions, which
    /// reach it by inspecting the active entry.
    ///
    /// # Safety
    /// `store_ptr` must be derived from a mutable borrow of the store that
    /// stays alive, and unreachable to every frame on this thread, until the
    /// guard is dropped.
    pub(crate) unsafe fn install(store_ptr: *mut StoreInner) -> StoreInstallGuard {
        let store_id = unsafe { store_ptr.as_ref().unwrap().objects.id() };
        // Nesting changes a pointer's provenance, never which store it
        // addresses, so the same store must arrive at the same address.
        debug_assert!(
            !Self::is_active(store_id)
                || STORE_CONTEXT_STACK.with(|cell| {
                    let stack = cell.borrow();
                    let active =
                        unsafe { stack.last().unwrap().entry.get().as_ref().unwrap().as_ptr() };
                    active == store_ptr
                }),
            "Store context pointer mismatch"
        );
        if Self::active_is_async(store_id) {
            return StoreInstallGuard { store_id: None };
        }
        Self::push(store_id, StoreContextEntry::Sync(store_ptr));
        StoreInstallGuard {
            store_id: Some(store_id),
        }
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
    pub(crate) unsafe fn pause(id: StoreId) -> StorePtrPauseGuard {
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
    pub(crate) fn try_get_current_unborrowed() -> Option<StorePtrWrapper> {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let top = stack.last_mut()?;
            if top.borrow_count != 0 {
                return None;
            }
            top.borrow_count += 1;
            Some(StorePtrWrapper {
                store_ptr: unsafe { top.entry.get().as_mut().unwrap().as_ptr() },
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
                StoreContextEntry::Async { guard, ptr } => {
                    GetStoreAsyncGuardResult::Ok(StoreAsyncGuardWrapper {
                        guard: guard as *mut _,
                        store_ptr: *ptr,
                    })
                }
                StoreContextEntry::Sync(ptr) => {
                    GetStoreAsyncGuardResult::NotAsync(StorePtrWrapper { store_ptr: *ptr })
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
/// must own the coroutine (see [`Cothread::new`]).
///
/// Every coroutine that can suspend while a call into Wasm is on its stack
/// must be a `Cothread`, and be resumed only through [`Cothread::resume`].
/// A `Cothread` stays on the thread that made it.
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
}

#[cfg(feature = "unsafe-cothread")]
impl CothreadState {
    fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.activation.is_empty()
    }

    /// Puts the state on the thread until the returned guard is dropped.
    ///
    /// # Panics
    /// Panics if a store whose call the coroutine is suspended in is on the
    /// thread's context stack: something on the thread is using it.
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
                    !stack.iter().any(|ctx| ctx.id == entry.id),
                    "a store the coroutine is suspended in is already on the current \
                     thread's context stack"
                );
            }
            let base = stack.len();
            stack.extend(self.entries.drain(..));
            base
        });
        // Safety: the first of a pair; the guard swaps again as soon as
        // control is back from the coroutine, however it comes back.
        unsafe { self.activation.swap() };
        InstalledCothreadState { state: self, base }
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
}

#[cfg(feature = "unsafe-cothread")]
impl Drop for InstalledCothreadState<'_> {
    fn drop(&mut self) {
        // Safety: the second of the pair `install` started.
        unsafe { self.state.activation.swap() };
        // Whatever is left above `base` belongs to the calls the coroutine
        // is suspended in. A coroutine that returned or unwound left nothing
        // behind: each of its calls took its entry and its trap handler off
        // on the way out.
        let (base, entries) = (self.base, &mut self.state.entries);
        let taken = STORE_CONTEXT_STACK.try_with(|cell| {
            let mut stack = cell.try_borrow_mut().ok()?;
            (base <= stack.len()).then(|| entries.extend(stack.drain(base..)))
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
    /// Panics if a store whose call the coroutine is suspended in is on the
    /// current thread's context stack, as it is while another call on that
    /// store runs on the thread.
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
        // which runs the frames of the call: they need their own state.
        // Safety: dropping the coroutine is what runs while the state is
        // installed, and the coroutine is not used again.
        let _installed = (!self.state.is_empty()).then(|| unsafe { self.state.install() });
        unsafe { std::mem::ManuallyDrop::drop(&mut self.coroutine) };
    }
}

impl StorePtrWrapper {
    pub(crate) fn as_ref(&self) -> StoreRef<'_> {
        // Safety: the store_mut is always initialized unless the StoreMutWrapper
        // is dropped, at which point it's impossible to call this function
        unsafe { self.store_ptr.as_ref().unwrap().as_store_ref() }
    }

    pub(crate) fn as_mut(&mut self) -> StoreMut<'_> {
        // Safety: the store_mut is always initialized unless the StoreMutWrapper
        // is dropped, at which point it's impossible to call this function
        unsafe { self.store_ptr.as_mut().unwrap().as_store_mut() }
    }
}

impl Clone for StorePtrWrapper {
    fn clone(&self) -> Self {
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
    fn drop(&mut self) {
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
        let id = self.as_mut().objects_mut().id();
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
        let id = unsafe { self.store_ptr.as_ref().unwrap().objects.id() };
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

impl Drop for StoreInstallGuard {
    fn drop(&mut self) {
        let Some(store_id) = self.store_id else {
            return;
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
                }
                (Some(top), true) => {
                    // If we're panicking and there's a store ID mismatch, just
                    // put the store back in the hope that its own install guard
                    // take care of uninstalling it later.
                    if top.id != store_id {
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
        let _ = shim.objects_mut().id();

        {
            // --- the shim calls back into the guest
            let inner_install =
                unsafe { StoreContext::install(shim.as_store_mut().inner as *mut _) };
            let pause = unsafe { StoreContext::pause(id) };

            // --- the inner import trampoline
            let mut inner_wrapper = unsafe { StoreContext::get_current(id) };
            let mut inner_shim = inner_wrapper.as_mut();
            let _ = inner_shim.objects_mut().id();
            drop(inner_wrapper);

            drop(pause);
            drop(inner_install);
        }

        // --- and goes on using the borrow it held throughout
        let _ = shim.objects_mut().id();

        drop(wrapper);
        drop(install);
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
        let _ = shim.objects_mut().id();

        shim.parked(|| {
            crate::Store::with_current(|lent| {
                let _ = lent.objects_mut().id();
            })
            .expect("a parked store is lendable");
        });

        // The lender goes back to the borrow it held throughout.
        let _ = shim.objects_mut().id();

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
        let _ = shim.objects_mut().id();

        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _inner_install =
                unsafe { StoreContext::install(shim.as_store_mut().inner as *mut _) };
            let _pause = unsafe { StoreContext::pause(id) };
            let mut inner_wrapper = unsafe { StoreContext::get_current(id) };
            let _ = inner_wrapper.as_mut().objects_mut().id();
            panic!("host function panicked");
        }));
        assert!(caught.is_err());

        let _ = shim.objects_mut().id();

        drop(wrapper);
        drop(install);
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
            let _install = unsafe { StoreContext::install(wrapper.as_mut().inner as *mut _) };
            let _pause = unsafe { StoreContext::pause(id) };
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
}
