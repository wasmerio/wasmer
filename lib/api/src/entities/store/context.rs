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
    ptr::NonNull,
};

#[cfg(feature = "experimental-async")]
use crate::LocalRwLockWriteGuard;

use super::{AsStoreMut, AsStoreRef, StoreInner, StoreMut, StoreRef};

use wasmer_types::StoreId;

enum StoreContextEntry {
    Sync(*mut StoreInner),

    #[cfg(feature = "experimental-async")]
    Async {
        guard: LocalRwLockWriteGuard<Box<StoreInner>>,
        /// Where `guard`'s store sits, taken once while nothing else could be
        /// borrowing it, so the identity checks below can compare addresses
        /// without deriving a borrow of their own. Deriving one would make a
        /// sibling of the borrow a paused frame is still holding, and creating
        /// it would invalidate that frame's.
        addr: *mut StoreInner,
    },
}

impl StoreContextEntry {
    /// Takes the store's address while the guard has just arrived and no other
    /// borrow of it can be outstanding.
    #[cfg(feature = "experimental-async")]
    fn new_async(mut guard: LocalRwLockWriteGuard<Box<StoreInner>>) -> Self {
        let addr = &mut **guard as *mut _;
        Self::Async { guard, addr }
    }

    /// The store this entry addresses, as a pointer every caller here goes on
    /// to write through.
    ///
    /// An async entry owns its store through a write guard, and the pointer has
    /// to be derived by `DerefMut`. Taken through `&self` the `Box` deref is a
    /// shared one, so what came back carried read-only provenance over the
    /// `StoreInner` and the `&mut` [`StorePtrWrapper::as_mut`] builds from it
    /// was undefined behaviour — see `borrow_provenance`'s
    /// `an_async_context_hands_out_a_writable_borrow`.
    fn as_ptr(&mut self) -> *mut StoreInner {
        match self {
            Self::Sync(ptr) => *ptr,
            #[cfg(feature = "experimental-async")]
            Self::Async { guard, .. } => &mut ***guard as *mut _,
        }
    }

    /// Where this entry's store sits, read without borrowing the store. Only
    /// good for identity checks; write through [`Self::as_ptr`].
    fn store_addr(&self) -> *mut StoreInner {
        match self {
            Self::Sync(ptr) => *ptr,
            #[cfg(feature = "experimental-async")]
            Self::Async { addr, .. } => *addr,
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
    /// The store `store_ptr` addresses. Kept so [`Drop`] needs no borrow: by
    /// then any later acquisition on the same entry has invalidated this
    /// pointer, and deriving from it to read the id is undefined behaviour.
    id: StoreId,
}

#[cfg(feature = "experimental-async")]
pub(crate) struct StoreAsyncGuardWrapper {
    pub(crate) guard: *mut LocalRwLockWriteGuard<Box<StoreInner>>,
    /// See [`StorePtrWrapper::id`].
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

pub(crate) struct StoreInstallGuard {
    /// `None` when this guard installed nothing, which happens only for a
    /// store an async context already holds — see [`StoreContext::install`].
    store_id: Option<StoreId>,
}

thread_local! {
    static STORE_CONTEXT_STACK: RefCell<Vec<StoreContext>> = const { RefCell::new(Vec::new()) };
}

impl StoreContext {
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

    #[cfg(feature = "unsafe-cothread")]
    fn push_cothread(id: StoreId, store_ptr: NonNull<StoreInner>) {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            stack.push(Self {
                id,
                borrow_count: 1,
                entry: UnsafeCell::new(StoreContextEntry::Sync(store_ptr.as_ptr())),
            });
        });
    }

    #[cfg(feature = "unsafe-cothread")]
    fn uninstall_cothread(id: StoreId) {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            // Search from the top so we find our entry even if the stack has
            // been disturbed (e.g. another guard panicked mid-flight).
            if let Some(pos) = stack.iter().rposition(|ctx| ctx.id == id) {
                stack.remove(pos);
            } else {
                panic!(
                    "CoroutineStoreGuard::drop: entry not found in context stack; \
                        the store context stack is corrupted"
                );
            }
        });
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
                    let active = unsafe {
                        stack
                            .last()
                            .unwrap()
                            .entry
                            .get()
                            .as_ref()
                            .unwrap()
                            .store_addr()
                    };
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
                ptr: unsafe { top.entry.get().as_ref().unwrap().store_addr() },
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
                id: top.id,
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
                id,
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
                StoreContextEntry::Async { guard, .. } => {
                    GetStoreAsyncGuardResult::Ok(StoreAsyncGuardWrapper {
                        guard: guard as *mut _,
                        id,
                    })
                }
                StoreContextEntry::Sync(ptr) => {
                    GetStoreAsyncGuardResult::NotAsync(StorePtrWrapper {
                        store_ptr: *ptr,
                        id,
                    })
                }
            }
        })
    }
}

#[cfg(feature = "unsafe-cothread")]
/// RAII guard that installs the store context on the thread-local stack when
/// created and removes it on drop. See [`crate::Store::coroutine_store_guard`].
pub struct CoroutineStoreGuard<'a> {
    store_id: StoreId,
    _store: std::marker::PhantomData<&'a mut StoreInner>,
}

#[cfg(feature = "unsafe-cothread")]
impl<'a> CoroutineStoreGuard<'a> {
    /// # Panics
    /// Panics if the store is anywhere on the current thread's context stack
    /// (active or suspended).
    ///
    /// # Safety
    /// Exactly one `StorePtrWrapper` derived from `store` must be alive on
    /// the suspended coroutine's stack for the duration of the guard.
    pub(crate) unsafe fn new(store: &'a mut StoreInner) -> Self {
        let store_id = store.objects.id();
        assert!(
            !StoreContext::is_active(store_id) && !StoreContext::is_suspended(store_id),
            "store is already on the current thread's context stack (active or suspended)"
        );
        StoreContext::push_cothread(store_id, NonNull::from(store));
        Self {
            store_id,
            _store: std::marker::PhantomData,
        }
    }
}

#[cfg(feature = "unsafe-cothread")]
impl Drop for CoroutineStoreGuard<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        StoreContext::uninstall_cothread(self.store_id);
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
                id: self.id,
            }
        })
    }
}

impl Drop for StorePtrWrapper {
    fn drop(&mut self) {
        if std::thread::panicking() {
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

#[cfg(feature = "experimental-async")]
impl Drop for StoreAsyncGuardWrapper {
    fn drop(&mut self) {
        if std::thread::panicking() {
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
            return;
        }
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let top = stack
                .last_mut()
                .expect("No store context installed on this thread");
            assert_eq!(top.id, self.store_id, "Mismatched store context access");
            assert_eq!(
                unsafe { top.entry.get().as_ref().unwrap() }.store_addr(),
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

    /// An async context hands out a borrow the `get_current` family then writes
    /// through, which is the one path [`StoreContextEntry::as_ptr`] cannot
    /// serve: taken through `&self`, its `Box` deref is a shared one, so the
    /// pointer it returns carries read-only provenance over the `StoreInner`.
    ///
    /// This is the flow a typed async host function takes on the `js` backend
    /// (`backend/js/entities/function/mod.rs`, where the closure installed by
    /// `new_with_env_async` calls `get_current` to convert its arguments) and
    /// the one `sys` takes in `async_runtime`'s `AsyncCallStoreMut` via
    /// `get_current_transient`. Both then write through what they get back.
    ///
    /// Like its siblings this passes natively; Miri is the point:
    ///
    /// ```text
    /// error: Undefined Behavior: trying to retag from <..> for Unique
    ///        permission, but that tag only grants SharedReadOnly permission
    ///        for this location
    ///   --> lib/api/src/entities/store/context.rs   &***guard as *const _ as *mut _
    /// ```
    ///
    /// Run it with:
    ///
    /// ```text
    /// cargo +nightly miri test -p wasmer --features sys,experimental-async \
    ///     --lib borrow_provenance
    /// ```
    #[test]
    #[cfg(feature = "experimental-async")]
    fn an_async_context_hands_out_a_writable_borrow() {
        let store = Store::default();
        let id = store.id();
        let store_async = store.into_async();

        // What `Function::call_async` installs before entering the guest: the
        // context owns the store through this write guard.
        let guard = store_async
            .inner
            .try_write()
            .expect("a fresh store is unlocked");
        let install = StoreContext::install_async(guard);

        // What an imported function's trampoline does to get its store back.
        let mut wrapper = unsafe { StoreContext::get_current(id) };
        let _ = wrapper.as_mut().objects_mut().id();

        drop(wrapper);
        drop(install);
    }

    /// The nesting in [`Self::nested_call_keeps_the_outer_borrow_usable`] rooted
    /// at an async entry: a synchronous import reached during an async guest
    /// call re-enters the guest, and uses its environment again afterwards.
    ///
    /// [`StoreContext::install`] installs nothing for a store an async context
    /// already holds, so an inner acquisition re-derives from the same write
    /// guard rather than nesting under the caller's. If the trampoline held the
    /// borrow it acquired, the two would be siblings and the inner one would
    /// invalidate the outer — and worse, the nested call can suspend, at which
    /// point the runtime releases the store and whoever takes it next
    /// invalidates the outer borrow anyway.
    ///
    /// So the trampoline holds no borrow: it keeps the entry borrowed with a
    /// [`StorePtrWrapper`] and hands the host function an environment that
    /// re-derives per access. This replays that flow; it fails if the
    /// environment goes back to holding a `StoreMut`.
    ///
    /// ```text
    /// cargo +nightly miri test -p wasmer --features sys,experimental-async \
    ///     --lib borrow_provenance
    /// ```
    // Reaches for the `sys` environment directly, since that is the backend
    // whose trampolines this replays.
    #[test]
    #[cfg(all(feature = "experimental-async", feature = "sys"))]
    fn a_nested_call_under_an_async_context_keeps_the_environment_usable() {
        let mut store = Store::default();
        let id = store.id();
        let env = crate::FunctionEnv::new(&mut store, 0u32);
        let store_async = store.into_async();

        let guard = store_async
            .inner
            .try_write()
            .expect("a fresh store is unlocked");
        let install = StoreContext::install_async(guard);

        // --- the import trampoline: the entry stays borrowed for as long as the
        //     host function runs, but no `StoreMut` is held.
        let _wrapper = unsafe { StoreContext::get_current(id) };
        let mut env_mut = unsafe {
            crate::backend::sys::entities::function::env::FunctionEnvMut::from_context(
                id,
                env.clone().into_sys(),
            )
        };
        *env_mut.data_mut() += 1;

        {
            // --- the host function calls back into the guest
            let inner_install =
                unsafe { StoreContext::install(env_mut.as_store_mut().inner as *mut _) };
            let pause = unsafe { StoreContext::pause(id) };

            // --- an import reached by that call touches the same environment
            let mut inner_wrapper = unsafe { StoreContext::get_current(id) };
            let mut inner_shim = inner_wrapper.as_mut();
            *env.as_mut(&mut inner_shim) += 1;
            drop(inner_wrapper);

            drop(pause);
            drop(inner_install);
        }

        // --- and goes on using its environment
        *env_mut.data_mut() += 1;
        assert_eq!(*env_mut.data(), 3);

        drop(_wrapper);
        drop(install);
    }
}
