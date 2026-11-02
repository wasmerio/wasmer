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
    cell::{Cell, RefCell, UnsafeCell},
    marker::PhantomData,
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
    Async(LocalRwLockWriteGuard<Box<StoreInner>>),
}

impl StoreContextEntry {
    fn as_ptr(&self) -> *mut StoreInner {
        match self {
            Self::Sync(ptr) => *ptr,
            #[cfg(feature = "experimental-async")]
            Self::Async(guard) => &***guard as *const _ as *mut _,
        }
    }
}

pub(crate) struct StoreContext {
    id: StoreId,
    installation: StoreContextId,

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
    store_id: StoreId,
    installation: StoreContextId,
}

/// Distinguishes nested installations of the same Store and pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StoreContextId(u64, PhantomData<*mut ()>);

#[cfg(feature = "experimental-async")]
pub(crate) struct StoreAsyncGuardWrapper {
    store_id: StoreId,
    installation: StoreContextId,
}

pub(crate) struct StorePtrPauseGuard {
    store_id: StoreId,
    installation: StoreContextId,
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
    installation: StoreContextId,
}

pub(crate) struct StoreInstallGuard {
    /// `None` when this guard installed nothing, which happens only for a
    /// store an async context already holds — see [`StoreContext::install`].
    installation: Option<(StoreId, StoreContextId)>,
}

thread_local! {
    static STORE_CONTEXT_STACK: RefCell<Vec<StoreContext>> = const { RefCell::new(Vec::new()) };
    static NEXT_STORE_CONTEXT_ID: Cell<u64> = const { Cell::new(0) };
}

impl StoreContext {
    fn is_active(id: StoreId) -> bool {
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

    pub(crate) fn new_installation_id() -> StoreContextId {
        NEXT_STORE_CONTEXT_ID.with(|next| {
            let id = next.get();
            next.set(id.checked_add(1).expect("Store context identity exhausted"));
            StoreContextId(id, PhantomData)
        })
    }

    fn push(id: StoreId, entry: StoreContextEntry) -> StoreContextId {
        let installation = Self::new_installation_id();
        Self::push_with_id(id, installation, entry);
        installation
    }

    fn push_with_id(id: StoreId, installation: StoreContextId, entry: StoreContextEntry) {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            stack.push(Self {
                id,
                installation,
                borrow_count: 0,
                entry: UnsafeCell::new(entry),
            });
        });
    }

    #[cfg(feature = "unsafe-cothread")]
    fn push_cothread(id: StoreId, store_ptr: NonNull<StoreInner>) -> StoreContextId {
        let installation = Self::push(id, StoreContextEntry::Sync(store_ptr.as_ptr()));
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            stack.last_mut().unwrap().borrow_count = 1;
        });
        installation
    }

    #[cfg(feature = "unsafe-cothread")]
    fn uninstall_cothread(id: StoreId, installation: StoreContextId) {
        let removed = STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            // Search from the top so we find our entry even if the stack has
            // been disturbed (e.g. another guard panicked mid-flight).
            if let Some(pos) = stack
                .iter()
                .rposition(|ctx| ctx.installation == installation)
            {
                assert_eq!(stack[pos].id, id, "Mismatched store context uninstall");
                stack.remove(pos)
            } else {
                panic!(
                    "CoroutineStoreGuard::drop: entry not found in context stack; \
                        the store context stack is corrupted"
                );
            }
        });
        drop(removed);
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
    #[cfg(all(feature = "experimental-async", any(feature = "js", test)))]
    pub(crate) fn install_async(
        guard: LocalRwLockWriteGuard<Box<StoreInner>>,
    ) -> ForcedStoreInstallGuard {
        Self::install_async_with_id(guard, Self::new_installation_id())
    }

    /// A suspended native invocation keeps its logical installation identity.
    #[cfg(feature = "experimental-async")]
    pub(crate) fn install_async_with_id(
        guard: LocalRwLockWriteGuard<Box<StoreInner>>,
        installation: StoreContextId,
    ) -> ForcedStoreInstallGuard {
        STORE_CONTEXT_STACK.with(|cell| {
            assert!(
                !cell
                    .borrow()
                    .iter()
                    .any(|ctx| ctx.installation == installation),
                "Store installation is already active or suspended on this thread"
            );
        });
        let store_id = guard.objects.id();
        Self::push_with_id(store_id, installation, StoreContextEntry::Async(guard));
        ForcedStoreInstallGuard {
            store_id,
            installation,
        }
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
                StoreContextEntry::Async(_) => true,
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
            return StoreInstallGuard { installation: None };
        }
        let installation = Self::push(store_id, StoreContextEntry::Sync(store_ptr));
        StoreInstallGuard {
            installation: Some((store_id, installation)),
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
                installation: top.installation,
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
                store_id: top.id,
                installation: top.installation,
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
                store_id: id,
                installation: top.installation,
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
                store_id: id,
                installation: top.installation,
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
                StoreContextEntry::Async(_) => {
                    GetStoreAsyncGuardResult::Ok(StoreAsyncGuardWrapper {
                        store_id: id,
                        installation: top.installation,
                    })
                }
                StoreContextEntry::Sync(ptr) => {
                    GetStoreAsyncGuardResult::NotAsync(StorePtrWrapper {
                        store_ptr: *ptr,
                        store_id: id,
                        installation: top.installation,
                    })
                }
            }
        })
    }

    fn assert_installation(&self, id: StoreId, installation: StoreContextId) {
        assert_eq!(self.id, id, "Mismatched store context access");
        assert_eq!(
            self.installation, installation,
            "Mismatched Store installation"
        );
    }

    fn release_borrow(id: StoreId, installation: StoreContextId) {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let top = stack
                .last_mut()
                .expect("No store context installed on this thread");
            top.assert_installation(id, installation);
            top.borrow_count = top
                .borrow_count
                .checked_sub(1)
                .expect("Store context borrow count underflow");
        });
    }

    fn uninstall(id: StoreId, installation: StoreContextId) {
        let removed = STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let Some(top) = stack.last() else {
                assert!(std::thread::panicking(), "Store context stack underflow");
                return None;
            };
            if std::thread::panicking() {
                // Another guard owns a different installation even when both
                // installations address the same Store.
                if top.installation != installation {
                    return None;
                }
            } else {
                top.assert_installation(id, installation);
                assert_eq!(
                    top.borrow_count, 0,
                    "Cannot uninstall store context while it is still borrowed"
                );
            }
            stack.pop()
        });
        // Unlocking an async Store can wake arbitrary code that reenters TLS.
        drop(removed);
    }
}

#[cfg(feature = "unsafe-cothread")]
/// RAII guard that installs the store context on the thread-local stack when
/// created and removes it on drop. See [`crate::Store::coroutine_store_guard`].
pub struct CoroutineStoreGuard<'a> {
    store_id: StoreId,
    installation: StoreContextId,
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
        let installation = StoreContext::push_cothread(store_id, NonNull::from(store));
        Self {
            store_id,
            installation,
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
        StoreContext::uninstall_cothread(self.store_id, self.installation);
    }
}

impl StorePtrWrapper {
    fn assert_current(&self) {
        STORE_CONTEXT_STACK.with(|cell| {
            cell.borrow()
                .last()
                .expect("No store context installed on this thread")
                .assert_installation(self.store_id, self.installation);
        });
    }

    pub(crate) fn as_ref(&self) -> StoreRef<'_> {
        self.assert_current();
        // Safety: the store_mut is always initialized unless the StoreMutWrapper
        // is dropped, at which point it's impossible to call this function
        unsafe { self.store_ptr.as_ref().unwrap().as_store_ref() }
    }

    pub(crate) fn as_mut(&mut self) -> StoreMut<'_> {
        self.assert_current();
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
            top.assert_installation(self.store_id, self.installation);
            match unsafe { top.entry.get().as_ref().unwrap() } {
                StoreContextEntry::Sync(ptr) if *ptr == self.store_ptr => (),
                _ => panic!("Mismatched store context access"),
            }
            top.borrow_count += 1;
            Self {
                store_ptr: self.store_ptr,
                store_id: self.store_id,
                installation: self.installation,
            }
        })
    }
}

impl Drop for StorePtrWrapper {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        StoreContext::release_borrow(self.store_id, self.installation);
    }
}

#[cfg(feature = "experimental-async")]
impl StoreAsyncGuardWrapper {
    #[cfg(any(feature = "sys", test))]
    pub(crate) fn as_mut(&mut self) -> StoreMut<'_> {
        let ptr = STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let top = stack
                .last_mut()
                .expect("No store context installed on this thread");
            top.assert_installation(self.store_id, self.installation);
            match unsafe { &mut *top.entry.get() } {
                StoreContextEntry::Async(guard) => &mut ***guard as *mut StoreInner,
                StoreContextEntry::Sync(_) => panic!("Mismatched async store context access"),
            }
        });
        // SAFETY: this wrapper retains a borrow of this exact Async installation.
        // The returned reference cannot outlive its mutable borrow of the wrapper.
        unsafe { StoreMut::from_raw(ptr) }
    }

    pub(crate) fn lock_handle(&self) -> crate::LocalRwLock<Box<StoreInner>> {
        STORE_CONTEXT_STACK.with(|cell| {
            let stack = cell.borrow();
            let top = stack
                .last()
                .expect("No store context installed on this thread");
            top.assert_installation(self.store_id, self.installation);
            match unsafe { &*top.entry.get() } {
                StoreContextEntry::Async(guard) => LocalRwLockWriteGuard::lock_handle(guard),
                StoreContextEntry::Sync(_) => panic!("Mismatched async store context access"),
            }
        })
    }
}

#[cfg(feature = "experimental-async")]
impl Drop for StoreAsyncGuardWrapper {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        StoreContext::release_borrow(self.store_id, self.installation);
    }
}

impl Drop for StoreInstallGuard {
    fn drop(&mut self) {
        if let Some((id, installation)) = self.installation {
            StoreContext::uninstall(id, installation);
        }
    }
}

impl Drop for ForcedStoreInstallGuard {
    fn drop(&mut self) {
        StoreContext::uninstall(self.store_id, self.installation);
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
            top.assert_installation(self.store_id, self.installation);
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
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    #[cfg(feature = "sys")]
    fn a_typed_callback_without_an_environment_releases_its_store_borrow() {
        use crate::{Function, FunctionEnv, FunctionEnvMut, Instance, Module};

        let mut store = Store::default();
        let id = store.id();
        let no_env = Function::new_typed(&mut store, move |a: i32, b: i64| -> i64 {
            assert_eq!(
                Store::with_current(|store| store.objects_mut().id()),
                Some(id)
            );
            i64::from(a) + b
        });
        let env = FunctionEnv::new(&mut store, ());
        let with_env = Function::new_typed_with_env(
            &mut store,
            &env,
            |_: FunctionEnvMut<()>, value: i64| -> i64 {
                assert!(Store::with_current(|_| ()).is_none());
                value
            },
        );
        let module = Module::new(
            &store,
            r#"(module
                (import "proof" "no_env" (func $no_env (param i32 i64) (result i64)))
                (import "proof" "with_env" (func $with_env (param i64) (result i64)))
                (func (export "run") (result i64)
                    i32.const 7 i64.const 35 call $no_env call $with_env))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &crate::imports! { "proof" => { "no_env" => no_env, "with_env" => with_env } },
        )
        .unwrap();
        let run = instance
            .exports
            .get_typed_function::<(), i64>(&store, "run")
            .unwrap();
        assert_eq!(run.call(&mut store).unwrap(), 42);
        assert!(StoreContext::is_empty());
    }

    #[test]
    fn wrappers_reject_a_different_installation_at_the_same_address() {
        let mut store = Store::default();
        let id = store.id();
        let ptr = store.as_store_mut().inner as *mut _;
        let original = unsafe { StoreContext::install(ptr) };
        let mut wrapper = unsafe { StoreContext::get_current(id) };
        let saved = STORE_CONTEXT_STACK.with(|cell| cell.borrow_mut().pop().unwrap());

        let replacement = unsafe { StoreContext::install(ptr) };
        let clone_rejected = catch_unwind(AssertUnwindSafe(|| wrapper.clone())).is_err();
        let access_rejected = catch_unwind(AssertUnwindSafe(|| {
            let _ = wrapper.as_mut();
        }))
        .is_err();
        let drop_rejected = catch_unwind(AssertUnwindSafe(|| drop(wrapper))).is_err();
        let replacement_borrows =
            STORE_CONTEXT_STACK.with(|cell| cell.borrow().last().unwrap().borrow_count);
        drop(replacement);

        // The rejected wrapper was consumed without touching either context's
        // bookkeeping. Restore and release its original installation explicitly.
        let installation = saved.installation;
        STORE_CONTEXT_STACK.with(|cell| cell.borrow_mut().push(saved));
        StoreContext::release_borrow(id, installation);
        drop(original);

        assert!(clone_rejected);
        assert!(access_rejected);
        assert!(drop_rejected);
        assert_eq!(replacement_borrows, 0);
        assert!(StoreContext::is_empty());
    }

    #[test]
    fn uninstall_rejects_a_different_installation_without_removing_it() {
        let mut store = Store::default();
        let id = store.id();
        let ptr = store.as_store_mut().inner as *mut _;
        let original = unsafe { StoreContext::install(ptr) };
        let saved = STORE_CONTEXT_STACK.with(|cell| cell.borrow_mut().pop().unwrap());
        let original_installation = saved.installation;
        let replacement = unsafe { StoreContext::install(ptr) };
        let replacement_installation = replacement.installation.unwrap().1;

        let rejected = catch_unwind(AssertUnwindSafe(|| drop(original))).is_err();
        let current =
            STORE_CONTEXT_STACK.with(|cell| cell.borrow().last().map(|entry| entry.installation));
        drop(replacement);
        STORE_CONTEXT_STACK.with(|cell| cell.borrow_mut().push(saved));
        StoreContext::uninstall(id, original_installation);

        assert!(rejected);
        assert_eq!(current, Some(replacement_installation));
        assert!(StoreContext::is_empty());
    }

    #[test]
    #[cfg(feature = "experimental-async")]
    fn async_wrapper_resolves_its_guard_after_context_vector_growth() {
        let store = Store::default().into_async();
        let root = StoreContext::install_async(store.inner.try_write().unwrap());
        let GetStoreAsyncGuardResult::Ok(mut wrapper) =
            (unsafe { StoreContext::try_get_current_async(store.id) })
        else {
            panic!("expected async context");
        };
        let capacity = STORE_CONTEXT_STACK.with(|cell| cell.borrow().capacity());
        let mut children = (0..capacity).map(|_| Store::default()).collect::<Vec<_>>();
        for child in &mut children {
            let ptr = child.as_store_mut().inner as *mut _;
            StoreContext::push(child.id(), StoreContextEntry::Sync(ptr));
        }
        let grew = STORE_CONTEXT_STACK.with(|cell| cell.borrow().capacity() > capacity);
        // No child wrapper exists, so removing these test installations cannot
        // invalidate any borrowed Store reference.
        for _ in &children {
            let child = STORE_CONTEXT_STACK.with(|cell| cell.borrow_mut().pop().unwrap());
            drop(child);
        }

        let resolved_id = wrapper.as_mut().objects_mut().id();
        let handle = wrapper.lock_handle();
        let held = handle.try_write().is_none();
        drop(wrapper);
        drop(root);
        assert!(grew);
        assert_eq!(resolved_id, store.id);
        assert!(held);
        assert!(handle.try_write().is_some());
        assert!(StoreContext::is_empty());
    }

    #[test]
    #[cfg(feature = "experimental-async")]
    fn uninstall_unlocks_outside_the_context_stack_borrow() {
        use std::{
            future::Future,
            sync::{
                Arc,
                atomic::{AtomicUsize, Ordering},
            },
            task::{Context, Wake, Waker},
        };

        struct CheckContext(AtomicUsize);
        impl Wake for CheckContext {
            fn wake(self: Arc<Self>) {
                assert!(StoreContext::is_empty());
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let store = Store::default().into_async();
        let root = StoreContext::install_async(store.inner.try_write().unwrap());
        let wake = Arc::new(CheckContext(AtomicUsize::new(0)));
        let waker = Waker::from(wake.clone());
        let mut context = Context::from_waker(&waker);
        let mut reader = Box::pin(store.inner.read());
        assert!(reader.as_mut().poll(&mut context).is_pending());

        drop(root);
        assert_eq!(wake.0.load(Ordering::SeqCst), 1);
        assert!(reader.as_mut().poll(&mut context).is_ready());
        assert!(StoreContext::is_empty());
    }

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
}
