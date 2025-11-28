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
//! Because async contexts can't be entered recursively (you
//! can't take a write lock twice, you have to use the existing
//! one), all code in this crate takes care to check for an
//! active store context first before trying to enter one. This
//! gives rise to the enums with cases for temporary locks vs
//! store context pointers, such as [`AsyncStoreReadLockInner`].
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

use crate::LocalRwLockWriteGuard;

use super::{AsStoreMut, AsStoreRef, StoreInner, StoreMut, StoreRef};

use wasmer_types::StoreId;

enum StoreContextEntry {
    Sync(*mut StoreInner),
    Async(LocalRwLockWriteGuard<StoreInner>),
}

impl StoreContextEntry {
    fn as_ptr(&self) -> *mut StoreInner {
        match self {
            Self::Sync(ptr) => *ptr,
            Self::Async(guard) => &**guard as *const _ as *mut _,
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

pub(crate) struct AsyncStoreGuardWrapper {
    pub(crate) guard: *mut LocalRwLockWriteGuard<StoreInner>,
}

pub(crate) enum GetAsyncStoreGuardResult {
    Ok(AsyncStoreGuardWrapper),
    NotAsync(StorePtrWrapper),
    NotInstalled,
}

pub(crate) struct ForcedStoreInstallGuard {
    store_id: StoreId,
}

pub(crate) enum StoreInstallGuard {
    Installed(StoreId),
    NotInstalled,
}

thread_local! {
    static STORE_CONTEXT_STACK: RefCell<Vec<StoreContext>> = const { RefCell::new(Vec::new()) };
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

    fn install(id: StoreId, entry: StoreContextEntry) {
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
    pub(crate) fn install_async(
        guard: LocalRwLockWriteGuard<StoreInner>,
    ) -> ForcedStoreInstallGuard {
        let store_id = guard.objects.id();
        Self::install(store_id, StoreContextEntry::Async(guard));
        ForcedStoreInstallGuard { store_id }
    }

    /// Install the store context as sync if it is not already installed.
    ///
    /// # Safety
    /// The pointer must be dereferenceable and remain valid until the
    /// store context is uninstalled.
    pub(crate) unsafe fn ensure_installed(store_ptr: *mut StoreInner) -> StoreInstallGuard {
        let store_id = unsafe { store_ptr.as_ref().unwrap().objects.id() };
        if Self::is_active(store_id) {
            StoreInstallGuard::NotInstalled
        } else {
            Self::install(store_id, StoreContextEntry::Sync(store_ptr));
            StoreInstallGuard::Installed(store_id)
        }
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
    pub(crate) unsafe fn try_get_current_async(id: StoreId) -> GetAsyncStoreGuardResult {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let Some(top) = stack.last_mut() else {
                return GetAsyncStoreGuardResult::NotInstalled;
            };
            if top.id != id {
                return GetAsyncStoreGuardResult::NotInstalled;
            }
            top.borrow_count += 1;
            match unsafe { top.entry.get().as_mut().unwrap() } {
                StoreContextEntry::Async(guard) => {
                    GetAsyncStoreGuardResult::Ok(AsyncStoreGuardWrapper {
                        guard: guard as *mut _,
                    })
                }
                StoreContextEntry::Sync(ptr) => {
                    GetAsyncStoreGuardResult::NotAsync(StorePtrWrapper { store_ptr: *ptr })
                }
            }
        })
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

impl Drop for StorePtrWrapper {
    fn drop(&mut self) {
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

impl Drop for AsyncStoreGuardWrapper {
    fn drop(&mut self) {
        let id = unsafe { self.guard.as_ref().unwrap().objects.id() };
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
        if let Self::Installed(store_id) = self {
            STORE_CONTEXT_STACK.with(|cell| {
                let mut stack = cell.borrow_mut();
                let top = stack.pop().expect("Store context stack underflow");
                assert_eq!(top.id, *store_id, "Mismatched store context uninstall");
                assert_eq!(
                    top.borrow_count, 0,
                    "Cannot uninstall store context while it is still borrowed"
                );
            })
        }
    }
}

impl Drop for ForcedStoreInstallGuard {
    fn drop(&mut self) {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let top = stack.pop().expect("Store context stack underflow");
            assert_eq!(top.id, self.store_id, "Mismatched store context uninstall");
            assert_eq!(
                top.borrow_count, 0,
                "Cannot uninstall store context while it is still borrowed"
            );
        })
    }
}
