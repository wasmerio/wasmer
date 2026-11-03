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

#[cfg(all(feature = "experimental-async", feature = "sys"))]
use std::rc::Rc;

enum StoreContextEntry {
    Sync(*mut StoreInner),

    #[cfg(feature = "experimental-async")]
    Async(LocalRwLockWriteGuard<Box<StoreInner>>),
    #[cfg(all(feature = "experimental-async", feature = "sys"))]
    Loan {
        pointer: *mut StoreInner,
        _guard: crate::backend::sys::async_runtime::NativeLoanWriteGuard,
    },
}

impl StoreContextEntry {
    fn as_ptr(&mut self) -> *mut StoreInner {
        match self {
            Self::Sync(ptr) => *ptr,
            #[cfg(feature = "experimental-async")]
            Self::Async(guard) => &mut ***guard as *mut _,
            #[cfg(all(feature = "experimental-async", feature = "sys"))]
            Self::Loan { pointer, .. } => *pointer,
        }
    }

    #[cfg(all(feature = "experimental-async", feature = "sys"))]
    fn owns_native_root(&self) -> bool {
        matches!(self, Self::Async(_) | Self::Loan { .. })
    }
}

pub(crate) struct StoreContext {
    id: StoreId,
    installation: StoreContextId,
    // Address comparison only; mutable access always resolves the entry anew.
    address: usize,
    #[cfg(all(feature = "experimental-async", feature = "sys"))]
    loan_epoch: Option<Rc<Cell<bool>>>,

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
    // A paused reference remains alive on the retained synchronous frame.
    paused_borrows: u32,
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
    installed: bool,
}

/// Exact synchronous installations owned by one suspended native stack.
#[cfg(all(feature = "experimental-async", feature = "sys"))]
#[derive(Default)]
pub(crate) struct SuspendedStoreContexts {
    entries: Vec<StoreContext>,
    prefix: Vec<StoreContextId>,
    root_depth: Option<usize>,
    root_paused_borrows: u32,
    owned_root: bool,
}

#[cfg(all(feature = "experimental-async", feature = "sys"))]
pub(crate) struct DetachedStoreContexts {
    caller: Vec<StoreContext>,
    previous_hidden_depth: Option<usize>,
}

pub(crate) struct StoreInstallGuard {
    /// `None` when this guard installed nothing, which happens only for a
    /// store an async context already holds — see [`StoreContext::install`].
    installation: Option<(StoreId, StoreContextId)>,
}

thread_local! {
    static STORE_CONTEXT_STACK: RefCell<Vec<StoreContext>> = const { RefCell::new(Vec::new()) };
    static NEXT_STORE_CONTEXT_ID: Cell<u64> = const { Cell::new(0) };
    #[cfg(all(feature = "experimental-async", feature = "sys"))]
    static HIDDEN_STORE_CONTEXT_DEPTH: Cell<usize> = const { Cell::new(0) };
}

impl StoreContext {
    #[cfg(all(feature = "experimental-async", feature = "sys"))]
    pub(crate) fn detach_current() -> DetachedStoreContexts {
        let previous_hidden_depth = STORE_CONTEXT_STACK.with(|stack| {
            (!stack.borrow().is_empty()).then(|| {
                HIDDEN_STORE_CONTEXT_DEPTH.with(|depth| {
                    let previous = depth.get();
                    depth.set(
                        previous
                            .checked_add(1)
                            .expect("hidden Store context depth exhausted"),
                    );
                    previous
                })
            })
        });
        DetachedStoreContexts {
            caller: STORE_CONTEXT_STACK.with(|stack| std::mem::take(&mut *stack.borrow_mut())),
            previous_hidden_depth,
        }
    }

    #[cfg(all(feature = "experimental-async", feature = "sys"))]
    pub(crate) fn async_root_admissible() -> bool {
        Self::is_empty() && HIDDEN_STORE_CONTEXT_DEPTH.with(|depth| depth.get() == 0)
    }
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

    fn push_with_id(id: StoreId, installation: StoreContextId, mut entry: StoreContextEntry) {
        let address = entry.as_ptr() as usize;
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            stack.push(Self {
                id,
                installation,
                address,
                #[cfg(all(feature = "experimental-async", feature = "sys"))]
                loan_epoch: None,
                borrow_count: 0,
                paused_borrows: 0,
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
            installed: true,
        }
    }

    #[cfg(all(feature = "experimental-async", feature = "sys"))]
    pub(crate) fn install_loan(
        guard: crate::backend::sys::async_runtime::NativeLoanWriteGuard,
        installation: StoreContextId,
        store_id: StoreId,
    ) -> ForcedStoreInstallGuard {
        assert_eq!(guard.store().objects.id(), store_id);
        let pointer = guard.context_pointer();
        STORE_CONTEXT_STACK.with(|cell| {
            assert!(
                !cell
                    .borrow()
                    .iter()
                    .any(|ctx| ctx.installation == installation)
            );
        });
        Self::push_with_id(
            store_id,
            installation,
            StoreContextEntry::Loan {
                pointer,
                _guard: guard,
            },
        );
        ForcedStoreInstallGuard {
            store_id,
            installation,
            installed: true,
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
                #[cfg(all(feature = "experimental-async", feature = "sys"))]
                StoreContextEntry::Loan { .. } => false,
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
                    stack.last().unwrap().address == store_ptr as usize
                }),
            "Store context pointer mismatch"
        );
        if Self::active_is_async(store_id) {
            return StoreInstallGuard { installation: None };
        }
        unsafe { Self::install_borrowed(store_ptr) }
    }

    /// Installs the exact mutable borrow supplied by a native synchronous call.
    /// Unlike the native async root's transient adapter, it may not reuse an
    /// enclosing Async guard's pointer while the caller retains its own borrow.
    ///
    /// # Safety
    /// The pointer's mutable borrow must remain alive and inaccessible until
    /// the returned guard is dropped, as for `install`.
    pub(crate) unsafe fn install_borrowed(store_ptr: *mut StoreInner) -> StoreInstallGuard {
        let store_id = unsafe { store_ptr.as_ref().unwrap().objects.id() };
        let installation = Self::push(store_id, StoreContextEntry::Sync(store_ptr));
        StoreInstallGuard {
            installation: Some((store_id, installation)),
        }
    }

    #[cfg(all(feature = "experimental-async", feature = "sys"))]
    pub(crate) fn native_root_entry(installation: StoreContextId) -> StoreInstallGuard {
        STORE_CONTEXT_STACK.with(|cell| {
            let stack = cell.borrow();
            let top = stack.last().expect("native root is installed");
            assert_eq!(top.installation, installation, "native root changed");
            assert!(unsafe { &*top.entry.get() }.owns_native_root());
        });
        StoreInstallGuard { installation: None }
    }

    #[cfg(all(feature = "experimental-async", feature = "sys"))]
    pub(crate) fn borrowed_loan_epoch(
        installation: StoreContextId,
        store_id: StoreId,
    ) -> Option<(StoreContextId, Rc<Cell<bool>>)> {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let depth = stack
                .iter()
                .position(|entry| entry.installation == installation)?;
            let root = unsafe { &*stack[depth].entry.get() };
            if (stack.len() <= depth + 1 && !matches!(root, StoreContextEntry::Loan { .. }))
                || stack[depth].id != store_id
                || !root.owns_native_root()
                || !stack[depth + 1..].iter().all(|entry| {
                    entry.id == store_id
                        && matches!(unsafe { &*entry.entry.get() }, StoreContextEntry::Sync(_))
                })
            {
                return None;
            }
            let top = stack.last_mut().unwrap();
            let epoch = top
                .loan_epoch
                .get_or_insert_with(|| Rc::new(Cell::new(true)))
                .clone();
            Some((top.installation, epoch))
        })
    }

    #[cfg(all(feature = "experimental-async", feature = "sys"))]
    pub(crate) fn can_suspend_borrowed_root(installation: StoreContextId) -> bool {
        STORE_CONTEXT_STACK.with(|cell| {
            let stack = cell.borrow();
            let Some(depth) = stack
                .iter()
                .position(|entry| entry.installation == installation)
            else {
                return false;
            };
            let root = &stack[depth];
            let top = stack.last().unwrap();
            let entry = unsafe { &*root.entry.get() };
            (stack.len() > depth + 1 || matches!(entry, StoreContextEntry::Loan { .. }))
                && entry.owns_native_root()
                && top.borrow_count == 0
                && top.paused_borrows == 0
                && stack[depth + 1..].iter().all(|entry| {
                    entry.id == root.id
                        && matches!(unsafe { &*entry.entry.get() }, StoreContextEntry::Sync(_))
                })
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
    pub(crate) unsafe fn pause(id: StoreId) -> StorePtrPauseGuard {
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let top = stack
                .last_mut()
                .expect("No store context installed on this thread");
            assert_eq!(top.id, id, "Mismatched store context access");
            let ref_count_decremented = if top.borrow_count > 0 {
                top.borrow_count -= 1;
                top.paused_borrows += 1;
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
                #[cfg(feature = "sys")]
                StoreContextEntry::Loan { pointer, .. } => {
                    GetStoreAsyncGuardResult::NotAsync(StorePtrWrapper {
                        store_ptr: *pointer,
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

    #[cfg(all(feature = "experimental-async", feature = "sys"))]
    pub(crate) fn can_suspend_suffix(installation: StoreContextId) -> bool {
        STORE_CONTEXT_STACK.with(|cell| {
            let stack = cell.borrow();
            let Some(depth) = stack
                .iter()
                .position(|ctx| ctx.installation == installation)
            else {
                return false;
            };
            // The existing same-Store path has no synchronous suffix. Its
            // retained-borrow ownership requires a separate Store loan mechanism.
            stack.len() == depth + 1 || Self::suspendable_suffix(&stack, depth)
        })
    }

    #[cfg(all(feature = "experimental-async", feature = "sys"))]
    fn suspendable_suffix(stack: &[Self], depth: usize) -> bool {
        let root = &stack[depth];
        matches!(unsafe { &*root.entry.get() }, StoreContextEntry::Async(_))
            && root.borrow_count == 0
            && root.paused_borrows == 0
            && stack[depth + 1..].iter().all(|ctx| {
                ctx.borrow_count == 0
                    && ctx.paused_borrows == 0
                    && ctx.id != root.id
                    && matches!(unsafe { &*ctx.entry.get() }, StoreContextEntry::Sync(_))
            })
    }
}

#[cfg(all(feature = "experimental-async", feature = "sys"))]
impl ForcedStoreInstallGuard {
    pub(crate) fn suspend_borrowed_root(&mut self, suspended: &mut SuspendedStoreContexts) {
        assert!(StoreContext::can_suspend_borrowed_root(self.installation));
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let depth = stack
                .iter()
                .position(|entry| entry.installation == self.installation)
                .expect("native Store root is installed");
            assert!(
                self.installed && suspended.root_depth.is_none() && suspended.entries.is_empty()
            );
            suspended
                .prefix
                .extend(stack[..depth].iter().map(|entry| entry.installation));
            suspended.root_depth = Some(depth);
            suspended.owned_root = true;
            suspended.entries = stack.split_off(depth);
            self.installed = false;
        });
    }

    pub(crate) fn restore_borrowed_root(suspended: &mut SuspendedStoreContexts) -> Self {
        assert!(suspended.owned_root);
        let depth = suspended.root_depth.expect("borrowed native root depth");
        let root = suspended
            .entries
            .first()
            .expect("borrowed native root retained");
        assert!(unsafe { &*root.entry.get() }.owns_native_root());
        let store_id = root.id;
        let installation = root.installation;
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            assert_eq!(stack.len(), depth, "borrowed native prefix depth changed");
            assert!(
                stack
                    .iter()
                    .map(|entry| entry.installation)
                    .eq(suspended.prefix.iter().copied()),
                "borrowed native prefix installations changed"
            );
            stack.append(&mut suspended.entries);
        });
        suspended.root_depth = None;
        suspended.prefix.clear();
        suspended.owned_root = false;
        Self {
            store_id,
            installation,
            installed: true,
        }
    }

    pub(crate) fn suspend_suffix(&self, suspended: &mut SuspendedStoreContexts) {
        assert!(!suspended.owned_root);
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let depth = stack
                .iter()
                .position(|ctx| ctx.installation == self.installation)
                .expect("native Store root is not installed");
            assert!(suspended.root_depth.is_none() && suspended.entries.is_empty());
            // Checked before physical suspension; no callback may run before
            // this transfer of the original derived pointers and installations.
            if stack.len() > depth + 1 {
                assert!(StoreContext::suspendable_suffix(&stack, depth));
            }
            suspended
                .prefix
                .extend(stack[..depth].iter().map(|ctx| ctx.installation));
            suspended.root_depth = Some(depth);
            suspended.root_paused_borrows = stack[depth].paused_borrows;
            suspended.entries = stack.split_off(depth + 1);
        });
    }

    pub(crate) fn restore_suffix(&self, suspended: &mut SuspendedStoreContexts) {
        assert!(!suspended.owned_root);
        let Some(depth) = suspended.root_depth else {
            return;
        };
        STORE_CONTEXT_STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            assert_eq!(stack.len(), depth + 1, "native Store prefix changed");
            stack[depth].assert_installation(self.store_id, self.installation);
            assert_eq!(stack[depth].paused_borrows, 0);
            assert!(
                stack[..depth]
                    .iter()
                    .map(|ctx| ctx.installation)
                    .eq(suspended.prefix.iter().copied()),
                "native Store prefix installations changed"
            );
            stack[depth].paused_borrows = std::mem::take(&mut suspended.root_paused_borrows);
            stack.append(&mut suspended.entries);
            suspended.prefix.clear();
            suspended.root_depth = None;
        });
    }
}

#[cfg(all(feature = "experimental-async", feature = "sys"))]
impl SuspendedStoreContexts {
    pub(crate) fn owns_root(&self) -> bool {
        self.owned_root
    }

    pub(crate) fn owned_root_identity(&self) -> Option<(StoreId, StoreContextId)> {
        if !self.owned_root {
            return None;
        }
        let root = self.entries.first()?;
        if !unsafe { &*root.entry.get() }.owns_native_root() {
            return None;
        }
        Some((root.id, root.installation))
    }

    pub(crate) fn borrowed_pointer(
        &self,
        scope: StoreContextId,
        store_id: StoreId,
    ) -> Option<*mut StoreInner> {
        if !self.owned_root
            || !self
                .entries
                .iter()
                .any(|entry| entry.installation == scope && entry.id == store_id)
        {
            return None;
        }
        let deepest = self.entries.last()?;
        if deepest.id != store_id {
            return None;
        }
        match unsafe { &*deepest.entry.get() } {
            StoreContextEntry::Sync(pointer) => Some(*pointer),
            StoreContextEntry::Loan { pointer, .. } => Some(*pointer),
            StoreContextEntry::Async(_) => None,
        }
    }
}

#[cfg(all(feature = "experimental-async", feature = "sys"))]
impl Drop for StoreContext {
    fn drop(&mut self) {
        if let Some(epoch) = &self.loan_epoch {
            epoch.set(false);
        }
    }
}

#[cfg(all(feature = "experimental-async", feature = "sys"))]
impl Drop for DetachedStoreContexts {
    fn drop(&mut self) {
        let remaining = STORE_CONTEXT_STACK.with(|stack| {
            std::mem::replace(&mut *stack.borrow_mut(), std::mem::take(&mut self.caller))
        });
        let empty = remaining.is_empty();
        if let Some(previous) = self.previous_hidden_depth {
            HIDDEN_STORE_CONTEXT_DEPTH.with(|depth| {
                assert_eq!(
                    depth.get(),
                    previous.checked_add(1).unwrap(),
                    "hidden Store contexts restored out of order"
                );
                depth.set(previous);
            });
        }
        // Unlock/wake and all owned payload destruction stay outside TLS's borrow.
        drop(remaining);
        if !std::thread::panicking() {
            assert!(empty, "retiring native root left Store contexts installed");
        }
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
                #[cfg(all(feature = "experimental-async", feature = "sys"))]
                StoreContextEntry::Loan { pointer, .. } if *pointer == self.store_ptr => (),
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
                #[cfg(feature = "sys")]
                StoreContextEntry::Loan { .. } => panic!("Mismatched async store context access"),
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
                #[cfg(feature = "sys")]
                StoreContextEntry::Loan { .. } => panic!("Mismatched async store context access"),
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
        if self.installed {
            StoreContext::uninstall(self.store_id, self.installation);
        }
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
                top.paused_borrows -= 1;
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

    /// Like the synchronous provenance proofs, this test is intended for Miri.
    /// Mutable access must derive from the owned write guard, never a shared
    /// reference cast to a writable pointer.
    #[test]
    #[cfg(feature = "experimental-async")]
    fn a_current_async_wrapper_can_mutate_its_owned_store() {
        let store = Store::default().into_async();
        let root = StoreContext::install_async(store.inner.try_write().unwrap());
        let mut wrapper = unsafe { StoreContext::get_current(store.id) };
        {
            let mut caller = wrapper.as_mut();
            caller.on_called(|_| Ok(wasmer_types::OnCalledAction::Finish));
            let cap = caller.as_store_async().expect("installed async context");
            // Cap construction must copy through the caller's own reference,
            // without reborrowing the ancestor write guard behind its back.
            caller.on_called(|_| Ok(wasmer_types::OnCalledAction::Finish));
            assert_eq!(crate::AsStoreAsync::store_id(&cap), store.id);
            assert_eq!(caller.objects_mut().id(), store.id);
        }
        assert_eq!(wrapper.as_mut().objects_mut().id(), store.id);
        drop(wrapper);
        drop(root);
        let store = store.into_store().unwrap();
        assert!(store.inner.on_called.is_some());
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

#[cfg(all(test, feature = "experimental-async", feature = "sys"))]
mod native_suspension {
    use super::*;
    use crate::Store;

    #[test]
    fn nested_sync_installations_keep_their_pointers_and_identity() {
        let store = Store::default().into_async();
        let root_id = StoreContext::new_installation_id();
        let root = StoreContext::install_async_with_id(store.inner.try_write().unwrap(), root_id);
        let mut other = Store::default();
        let other_id = other.id();
        let other_ptr = other.as_store_mut().inner as *mut StoreInner;
        let nested = unsafe { StoreContext::install(other_ptr) };
        let mut third = Store::default();
        let inner = unsafe { StoreContext::install(third.as_store_mut().inner) };
        // A -> B -> C -> B: the repeated Store belongs to a distinct Sync
        // caller. Only A is unlocked; B and C remain owned by this stack.
        let repeated = unsafe { StoreContext::install(other_ptr) };
        let identities = STORE_CONTEXT_STACK.with(|stack| {
            stack
                .borrow()
                .iter()
                .map(|entry| entry.installation)
                .collect::<Vec<_>>()
        });
        assert_ne!(identities[1], identities[3]);
        assert!(StoreContext::can_suspend_suffix(root_id));
        let mut suspended = SuspendedStoreContexts::default();
        root.suspend_suffix(&mut suspended);
        drop(root);
        assert!(StoreContext::is_empty());

        let root = StoreContext::install_async_with_id(store.inner.try_write().unwrap(), root_id);
        root.restore_suffix(&mut suspended);
        STORE_CONTEXT_STACK.with(|stack| {
            assert!(
                stack
                    .borrow()
                    .iter()
                    .map(|entry| entry.installation)
                    .eq(identities)
            );
            assert_eq!(stack.borrow()[3].address, other_ptr as usize);
        });
        let mut borrowed = unsafe { StoreContext::get_current(other_id) };
        assert_eq!(borrowed.as_mut().objects_mut().id(), other_id);
        drop(borrowed);
        drop(repeated);
        drop(inner);
        drop(nested);
        // The original caller's borrow stays usable after the retained child
        // pointer was restored and accessed. This is also a Miri proof source.
        assert_eq!(other.as_store_mut().objects_mut().id(), other_id);
        drop(root);
        assert!(StoreContext::is_empty());
    }

    #[test]
    fn a_sync_suffix_rejects_borrows_root_aliases_and_extra_async_roots() {
        let store = Store::default().into_async();
        let root_id = StoreContext::new_installation_id();
        let root = StoreContext::install_async_with_id(store.inner.try_write().unwrap(), root_id);
        let borrowed = unsafe { StoreContext::get_current(store.id) };
        // No suffix: preserve the existing supported same-Store call path.
        assert!(StoreContext::can_suspend_suffix(root_id));
        let pause = unsafe { StoreContext::pause(store.id) };
        let mut other = Store::default();
        let nested = unsafe { StoreContext::install(other.as_store_mut().inner) };
        assert!(!StoreContext::can_suspend_suffix(root_id));
        drop(nested);
        drop(pause);
        drop(borrowed);

        let root_ptr = unsafe { StoreContext::get_current_transient(store.id) };
        let nested = unsafe { StoreContext::install(other.as_store_mut().inner) };
        let borrowed = unsafe { StoreContext::get_current(other.id()) };
        assert!(!StoreContext::can_suspend_suffix(root_id));
        let pause = unsafe { StoreContext::pause(other.id()) };
        assert!(!StoreContext::can_suspend_suffix(root_id));
        drop(pause);
        drop(borrowed);
        let repeated = unsafe { StoreContext::install(root_ptr) };
        assert!(!StoreContext::can_suspend_suffix(root_id));
        drop(repeated);
        drop(nested);
        let other = other.into_async();
        let nested = StoreContext::install_async(other.inner.try_write().unwrap());
        assert!(!StoreContext::can_suspend_suffix(root_id));
        drop(nested);
        drop(root);
        assert!(StoreContext::is_empty());
    }
}
