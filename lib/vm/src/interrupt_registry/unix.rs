#![cfg(unix)]

use std::{
    cell::UnsafeCell,
    ffi::CStr,
    os::unix::thread::RawPthread,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicUsize, Ordering},
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
    pthread: RawPthread,
    /// Whether this store was interrupted.
    interrupted: bool,
    /// See comments in [`ThreadInterruptState`].
    thread_current_signal_target_store: Arc<AtomicUsize>,
}

/// Thread-related state; only **PARTS** of this struct are safe to access
/// from within the interrupt handler.
struct ThreadInterruptState {
    /// We need to maintain a stack of active stores per thread, hence the vec.
    /// This should not be touched by the interrupt handler.
    active_stores: Vec<StoreId>,

    /// Always stores the top entry from `active_stores`. Needed since a vec is not
    /// safe to access from signal handlers.
    current_active_store: AtomicUsize,

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
        let pthread = unsafe { libc::pthread_self() };

        StoreInterruptState {
            pthread,
            interrupted: false,
            thread_current_signal_target_store,
        }
    });

    if store_state.interrupted {
        return Err(InstallError::AlreadyInterrupted);
    }

    THREAD_INTERRUPT_STATE.with(|t| {
        // Safety: See comments on THREAD_INTERRUPT_STATE.
        let borrow = unsafe { t.get().as_mut().unwrap() };
        borrow.active_stores.push(store_id);
        borrow
            .current_active_store
            .store(store_id.as_raw().get(), Ordering::Release);
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
        match borrow.active_stores.pop_if(|x| *x == store_id) {
            Some(_) => {
                borrow.current_active_store.store(
                    borrow
                        .active_stores
                        .last()
                        .map(|x| x.as_raw().get())
                        .unwrap_or(0),
                    Ordering::Release,
                );
                borrow.active_stores.contains(&store_id)
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
    let Entry::Occupied(mut store_state) = STORE_INTERRUPT_STATE.entry(store_id) else {
        return Err(InterruptError::StoreNotRunning);
    };
    let store_state = store_state.get_mut();

    if let Err(_) = store_state
        .thread_current_signal_target_store
        .compare_exchange(
            0,
            store_id.as_raw().get(),
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
    {
        return Err(InterruptError::OtherInterruptInProgress);
    }

    store_state.interrupted = true;

    unsafe {
        if libc::pthread_kill(store_state.pthread, libc::SIGUSR1) != 0 {
            let errno = *libc::__errno_location();
            let error_str = CStr::from_ptr(libc::strerror(errno)).to_str().unwrap();
            return Err(InterruptError::FailedToSendSignal(error_str));
        }
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
        if let Err(_) = state.current_signal_target_store.compare_exchange(
            current_signal_target_store,
            0,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            unreachable!("current_signal_target_store isn't changed unless it's zero");
        }

        current_active_store == current_signal_target_store
    })
}

/// Returns true if the store with the given ID has already been interrupted.
pub fn is_interrupted(store_id: StoreId) -> bool {
    let Entry::Occupied(store_state_entry) = STORE_INTERRUPT_STATE.entry(store_id) else {
        return false;
    };
    store_state_entry.get().interrupted
}
