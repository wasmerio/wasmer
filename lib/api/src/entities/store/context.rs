//! Thread-local storage for storing the current store context,
//! i.e. the currently active `StoreMut`(s). When a function is
//! called, an owned `StoreMut` value must be placed in the
//! store context, so it can be retrieved later when needed
//! (mainly when calling imported functions). We maintain a
//! stack because it is technically possible to have nested
//! `Function::call` invocations that use different stores,
//! such as:
//!     call(store1, func1) -> wasm code -> imported func ->
//!     call(store2, func2)
//!
//! Also note that this stack is maintained by both function
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

use super::{AsStoreMut, AsStoreRef, StoreMut};

use wasmer_types::StoreId;

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
    store_mut: UnsafeCell<StoreMut>,
}

pub(crate) struct StoreMutWrapper {
    store_mut: *mut StoreMut,
}

pub(crate) enum StoreInstallGuard<'a> {
    Installed {
        store_id: StoreId,
        store_mut: &'a mut dyn AsStoreMut,
    },
    NotInstalled,
}

thread_local! {
    static STORE_CONTEXT_STACK: RefCell<Vec<StoreContext>> = RefCell::new(Vec::new());
}

impl StoreContext {
    fn is_active(id: StoreId) -> bool {
        STORE_CONTEXT_STACK.with(|cell| {
            let stack = cell.borrow();
            stack.last().map_or(false, |ctx| ctx.id == id)
        })
    }

    fn is_suspended(id: StoreId) -> bool {
        STORE_CONTEXT_STACK.with(|cell| {
            let stack = cell.borrow();
            stack.iter().rev().skip(1).any(|ctx| ctx.id == id)
        })
    }

    fn install(store_mut: StoreMut) {
        // No need to scan through the list, only one StoreMut
        // can be active at any time because of the RwLock in
        // Store.
        let id = store_mut.objects().id();
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            stack.push(StoreContext {
                id,
                borrow_count: 0,
                store_mut: UnsafeCell::new(store_mut),
            });
        })
    }

    /// Ensure that a store context with the given id is installed.
    /// Returns true if the [`StoreMut`] was taken out of the provided
    /// [`AsStoreMut`] and installed, false if it was already active.
    pub(crate) fn ensure_installed<'a>(
        store_mut: &'a mut impl AsStoreMut,
    ) -> StoreInstallGuard<'a> {
        let store_id = store_mut.objects().id();
        if Self::is_active(store_id) {
            StoreInstallGuard::NotInstalled
        } else {
            let Some(store_mut_instance) = store_mut.take() else {
                if Self::is_suspended(store_id) {
                    // Impossible because you can't have two writable locks on the Store
                    unreachable!(
                        "Cannot install store context recursively. \
                        This should be impossible; please open an issue \
                        describing how you ran into this panic at
                        https://github.com/wasmerio/wasmer/issues/new/choose"
                    );
                }
                // Document the expected usage of Function::call here in case someone
                // does too many weird things since, without doing weird things, the
                // only way for embedder code to gain access to an AsStoreMut is by
                // going through Store::as_mut anyway.
                panic!(
                    "Failed to install store context because the provided AsStoreMut \
                    implementation does not own its StoreMut. The usual cause of this \
                    error is Function::call or Module::instantiate not being called \
                    with the output from Store::as_mut."
                );
            };
            Self::install(store_mut_instance);
            StoreInstallGuard::Installed {
                store_id,
                store_mut,
            }
        }
    }

    /// Safety: This method lets you borrow multiple mutable references
    /// to the currently active StoreMut. The caller must ensure that:
    ///   * there is only one mutable reference alive, or
    ///   * all but one mutable reference are inaccessible and passed
    ///     into a function that lost the reference (e.g. into WASM code)
    /// The intended, valid use-case for this method is from within
    /// imported function trampolines.
    pub(crate) unsafe fn get_current(id: StoreId) -> StoreMutWrapper {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let top = stack
                .last_mut()
                .expect("No store context installed on this thread");
            assert_eq!(top.id, id, "Mismatched store context access");
            top.borrow_count += 1;
            StoreMutWrapper {
                store_mut: top.store_mut.get(),
            }
        })
    }
}

impl StoreMutWrapper {
    pub(crate) fn as_mut(&mut self) -> &mut StoreMut {
        // Safety: the store_mut is always initialized unless the StoreMutWrapper
        // is dropped, at which point it's impossible to call this function
        unsafe { self.store_mut.as_mut().unwrap() }
    }
}

impl Drop for StoreMutWrapper {
    fn drop(&mut self) {
        let id = self.as_mut().objects().id();
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

impl Drop for StoreInstallGuard<'_> {
    fn drop(&mut self) {
        if let StoreInstallGuard::Installed {
            store_id,
            store_mut,
        } = self
        {
            STORE_CONTEXT_STACK.with(|cell| {
                let mut stack = cell.borrow_mut();
                let top = stack.pop().expect("Store context stack underflow");
                assert_eq!(top.id, *store_id, "Mismatched store context uninstall");
                assert_eq!(
                    top.borrow_count, 0,
                    "Cannot uninstall store context while it is still borrowed"
                );
                store_mut.put_back(top.store_mut.into_inner());
            })
        }
    }
}
