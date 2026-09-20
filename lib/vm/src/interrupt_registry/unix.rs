use std::{
    cell::UnsafeCell,
    ffi::CStr,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering},
    },
};

use dashmap::{DashMap, Entry};
use wasmer_types::StoreId;

use super::*;

/// All necessary data for interrupting a store running WASM code
/// on a thread.
struct StoreInterruptState {
    /// The pthread of the thread the store is running on, used to
    /// send the interrupt signal. Note that multiple stores may
    /// be executing WASM code within the same OS thread.
    ///
    /// We store this as a plain integer because `libc::pthread_t` is a raw
    /// pointer on some Unix targets, which would make the global `DashMap`
    /// fail its `Send` bounds even though we only treat the value as an opaque
    /// thread identifier.
    pthread: usize,
    /// Whether this store was interrupted.
    interrupted: Arc<AtomicBool>,
    /// Native waits that must be woken cooperatively before the signal can be
    /// observed back on the Wasm stack.
    wait_wakers: Vec<Arc<dyn InterruptWaitWaker>>,
    /// See comments in [`ThreadInterruptState`].
    thread_current_signal_target_store: Arc<AtomicUsize>,
}

/// Thread-related state; only **PARTS** of this struct are safe to access
/// from within the interrupt handler.
struct ThreadInterruptState {
    /// We need to maintain a stack of active stores per thread, hence the vec.
    /// This should not be touched by the interrupt handler.
    active_stores: Vec<(StoreId, Arc<AtomicBool>)>,

    /// Always stores the top entry from `active_stores`. Needed since a vec is not
    /// safe to access from signal handlers.
    current_active_store: AtomicUsize,

    /// Interrupt flag for `current_active_store`. This gives code executing on
    /// the Wasm stack a lock-free check: taking the global DashMap lock there
    /// would itself be unsafe to abandon from the signal handler.
    current_active_interrupted: AtomicPtr<AtomicBool>,

    /// Shared state between the thread requesting the interrupt
    /// and the thread running the store's code. The thread
    /// requesting the interrupt writes the ID of the store it
    /// wants to interrupt to this atomic. The interrupted
    /// thread later checks this value (through its own clone
    /// of the Arc in [`ThreadInterruptState`]) against the currently
    /// running store, and traps only if they match, recording the
    /// interrupt otherwise.
    /// Note that mutexes are not safe for use within signal
    /// handlers; only atomics can be safely used.
    current_signal_target_store: Arc<AtomicUsize>,
}

/// HashMap of all store states, accessible from all threads
static STORE_INTERRUPT_STATE: LazyLock<DashMap<StoreId, StoreInterruptState>> =
    LazyLock::new(Default::default);

thread_local! {
    /// Thread-local thread state. The book-keeping in a RefCell isn't
    /// guaranteed to be signal-handler-safe, so we use an UnsafeCell
    /// instead. The cell is only accessed in leaf functions, so it
    /// should be safe.
    /// The *only* actually unsafe access happens if a signal comes in
    /// while another function is modifying the cell; In this case,
    /// [`should_interrupt_now`] will return junk results. This is
    /// still safe because:
    ///   * `should_interrupt_now` only atomically accesses data from this cell
    ///   * junk results shouldn't matter if we're not running WASM code
    static THREAD_INTERRUPT_STATE: UnsafeCell<ThreadInterruptState> =
        UnsafeCell::new(ThreadInterruptState {
                    active_stores: vec![],
                    current_active_store: AtomicUsize::new(0),
                    current_active_interrupted: AtomicPtr::new(std::ptr::null_mut()),
                    current_signal_target_store: Arc::new(AtomicUsize::new(0)),
        });
}

/// Install interrupt state for the given store. Note that this function
/// may be called more than once, and correctly maintains a stack of
/// stores for which the state is installed.
pub fn install(store_id: StoreId) -> Result<InterruptInstallGuard, InstallError> {
    let store_state = STORE_INTERRUPT_STATE.entry(store_id).or_insert_with(|| {
        let thread_current_signal_target_store = THREAD_INTERRUPT_STATE.with(|t| {
            // Safety: See comments on THREAD_INTERRUPT_STATE.
            unsafe { t.get().as_mut().unwrap() }
                .current_signal_target_store
                .clone()
        });

        // TODO: isn't there a way to get this without reaching for libc APIs?
        // Since stores can't be sent across threads once they start executing code,
        // we don't need to update this value for recursive calls.
        #[allow(trivial_numeric_casts)]
        let pthread = unsafe { libc::pthread_self() as usize };

        StoreInterruptState {
            pthread,
            interrupted: Arc::new(AtomicBool::new(false)),
            wait_wakers: Vec::new(),
            thread_current_signal_target_store,
        }
    });

    if store_state.interrupted.load(Ordering::Acquire) {
        return Err(InstallError::AlreadyInterrupted);
    }

    let interrupted = store_state.interrupted.clone();

    THREAD_INTERRUPT_STATE.with(|t| {
        // Safety: See comments on THREAD_INTERRUPT_STATE.
        let borrow = unsafe { t.get().as_mut().unwrap() };
        borrow.active_stores.push((store_id, interrupted.clone()));
        borrow
            .current_active_store
            .store(store_id.as_raw().get(), Ordering::Release);
        borrow
            .current_active_interrupted
            .store(Arc::as_ptr(&interrupted).cast_mut(), Ordering::Release);
    });

    Ok(InterruptInstallGuard { store_id })
}

pub(super) fn uninstall(store_id: StoreId) {
    let Entry::Occupied(store_state_entry) = STORE_INTERRUPT_STATE.entry(store_id) else {
        panic!("Internal error: interrupt state not installed for store");
    };

    let has_more_installations = THREAD_INTERRUPT_STATE.with(|t| {
        // Safety: See comments on THREAD_INTERRUPT_STATE.
        let borrow = unsafe { t.get().as_mut().unwrap() };
        match borrow.active_stores.pop_if(|(id, _)| *id == store_id) {
            Some(_) => {
                borrow.current_active_store.store(
                    borrow
                        .active_stores
                        .last()
                        .map_or(0, |(id, _)| id.as_raw().get()),
                    Ordering::Release,
                );
                borrow.current_active_interrupted.store(
                    borrow
                        .active_stores
                        .last()
                        .map_or(std::ptr::null_mut(), |(_, flag)| {
                            Arc::as_ptr(flag).cast_mut()
                        }),
                    Ordering::Release,
                );
                borrow.active_stores.iter().any(|(id, _)| *id == store_id)
            }
            None => panic!("InterruptInstallGuard dropped out of order"),
        }
    });

    // If this store is still active at some other point within the
    // thread, we should keep its state around. Otherwise, it should
    // be deleted from the global interrupt state. Note that this will
    // also reset the `interrupted` flag, allowing the store to be used
    // for further function calls.
    if !has_more_installations {
        store_state_entry.remove();
    }
}

/// Interrupt the store with the given ID. Best effort is made to ensure
/// interrupts are handled. However, there is no guarantee; under rare
/// circumstances, it is possible for the interrupt to be missed. One such
/// case is when the target thread is about to call WASM code but has not
/// yet made the call.
///
/// To make sure the code is interrupted, the target thread should notify
/// the signalling thread that it has finished running in some way, and
/// the signalling thread must wait for that notification and retry the
/// interrupt if the notification is not received after some time.
pub fn interrupt(store_id: StoreId) -> Result<(), InterruptError> {
    let (wait_wakers, signal_error) = {
        let Entry::Occupied(mut store_state) = STORE_INTERRUPT_STATE.entry(store_id) else {
            return Err(InterruptError::StoreNotRunning);
        };
        let store_state = store_state.get_mut();

        if store_state
            .thread_current_signal_target_store
            .compare_exchange(
                0,
                store_id.as_raw().get(),
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_err()
        {
            return Err(InterruptError::OtherInterruptInProgress);
        }

        store_state.interrupted.store(true, Ordering::Release);
        let wait_wakers = store_state.wait_wakers.clone();
        let signal_error = unsafe {
            #[allow(trivial_numeric_casts)]
            let errno = libc::pthread_kill(store_state.pthread as libc::pthread_t, libc::SIGUSR1);
            if errno != 0 {
                let error_str = CStr::from_ptr(libc::strerror(errno)).to_str().unwrap();
                Some(InterruptError::FailedToSendSignal(error_str))
            } else {
                None
            }
        };
        (wait_wakers, signal_error)
    };

    // Do not hold a DashMap shard while waking. A waiter registers while
    // holding its address mutex so this ordering prevents both lost wakes and
    // a map/mutex lock inversion. Send the signal first so the waiter cannot
    // finish and release its pthread before pthread_kill observes it.
    for waker in wait_wakers {
        waker.wake();
    }

    if let Some(error) = signal_error {
        return Err(error);
    }

    Ok(())
}

/// Called from within the signal handler to decide whether we should interrupt
/// the currently running WASM code. This function *MAY* return junk results in
/// case a signal comes in during an install or uninstall operation. However,
/// in such cases, there is no WASM code running, and the result will be ignored
/// by the signal handler anyway.
pub(crate) fn on_interrupted() -> bool {
    THREAD_INTERRUPT_STATE.with(|t| {
        // Safety: See comments on THREAD_INTERRUPT_STATE.
        let state = unsafe { t.get().as_ref().unwrap() };

        let current_active_store = state.current_active_store.load(Ordering::Acquire);

        let current_signal_target_store = state.current_signal_target_store.load(Ordering::Acquire);
        assert_ne!(
            current_signal_target_store, 0,
            "current_signal_target_store should be set before signalling the WASM thread"
        );
        if state
            .current_signal_target_store
            .compare_exchange(
                current_signal_target_store,
                0,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_err()
        {
            unreachable!("current_signal_target_store isn't changed unless it's zero");
        }

        current_active_store == current_signal_target_store
    })
}

/// Returns true if the store with the given ID has already been interrupted.
pub fn is_interrupted(store_id: StoreId) -> bool {
    let current = THREAD_INTERRUPT_STATE.with(|t| {
        // Safety: only atomic fields are accessed, including from code that
        // may itself be interrupted by the signal handler.
        let state = unsafe { t.get().as_ref().unwrap() };
        if state.current_active_store.load(Ordering::Acquire) != store_id.as_raw().get() {
            return None;
        }
        let interrupted = state.current_active_interrupted.load(Ordering::Acquire);
        assert!(!interrupted.is_null());
        // The Arc is retained by active_stores until current_active_store is
        // changed during uninstall, when no Wasm code is executing.
        Some(unsafe { (*interrupted).load(Ordering::Acquire) })
    });
    if let Some(interrupted) = current {
        return interrupted;
    }

    let Entry::Occupied(store_state_entry) = STORE_INTERRUPT_STATE.entry(store_id) else {
        return false;
    };
    store_state_entry.get().interrupted.load(Ordering::Acquire)
}

pub(crate) fn register_wait(
    store_id: StoreId,
    waker: Arc<dyn InterruptWaitWaker>,
) -> Option<InterruptWaitGuard> {
    let Entry::Occupied(mut state) = STORE_INTERRUPT_STATE.entry(store_id) else {
        return None;
    };
    if state.get().interrupted.load(Ordering::Acquire) {
        return None;
    }
    state.get_mut().wait_wakers.push(waker.clone());
    Some(InterruptWaitGuard { store_id, waker })
}

pub(super) fn unregister_wait(store_id: StoreId, waker: &Arc<dyn InterruptWaitWaker>) {
    let Entry::Occupied(mut state) = STORE_INTERRUPT_STATE.entry(store_id) else {
        return;
    };
    state
        .get_mut()
        .wait_wakers
        .retain(|registered| !Arc::ptr_eq(registered, waker));
}
