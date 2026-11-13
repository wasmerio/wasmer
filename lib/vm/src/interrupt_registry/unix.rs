use std::{
    ffi::CStr,
    ptr,
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
    installations: usize,
    /// The thread's signal target, shared with the signal sender.
    thread_current_signal_target_store: Arc<AtomicUsize>,
}

/// Registrations stay live while an async call is suspended.
static STORE_INTERRUPT_STATE: LazyLock<DashMap<StoreId, StoreInterruptState>> =
    LazyLock::new(Default::default);

thread_local! {
    // The handler can run on a thread that has never entered interruptible Wasm.
    // Keep access to the signal target allocation-free even on those threads.
    static SIGNAL_TARGET: AtomicPtr<AtomicUsize> = const { AtomicPtr::new(ptr::null_mut()) };
    static SIGNAL_TARGET_OWNER: SignalTarget = {
        let target = Arc::new(AtomicUsize::new(0));
        SIGNAL_TARGET.with(|slot| slot.store(Arc::as_ptr(&target).cast_mut(), Ordering::Relaxed));
        SignalTarget(target)
    };
}

struct SignalTarget(Arc<AtomicUsize>);

impl Drop for SignalTarget {
    fn drop(&mut self) {
        SIGNAL_TARGET.with(|slot| slot.store(ptr::null_mut(), Ordering::Relaxed));
    }
}

/// Install interrupt state for the given store. Multiple calls on the same
/// thread can keep the store registered until the last call finishes.
pub fn install(store_id: StoreId) -> Result<InterruptInstallGuard, InstallError> {
    let mut store_state = STORE_INTERRUPT_STATE.entry(store_id).or_insert_with(|| {
        let thread_current_signal_target_store =
            SIGNAL_TARGET_OWNER.with(|target| Arc::clone(&target.0));

        #[allow(trivial_numeric_casts)]
        let pthread = unsafe { libc::pthread_self() as usize };

        StoreInterruptState {
            pthread,
            interrupted: Arc::default(),
            installations: 0,
            thread_current_signal_target_store,
        }
    });

    if store_state.interrupted.load(Ordering::Acquire) {
        return Err(InstallError::AlreadyInterrupted);
    }

    store_state.installations += 1;
    Ok(InterruptInstallGuard {
        store_id,
        interrupted: Arc::clone(&store_state.interrupted),
    })
}

pub(super) fn uninstall(store_id: StoreId) {
    let Entry::Occupied(mut store_state) = STORE_INTERRUPT_STATE.entry(store_id) else {
        panic!("Internal error: interrupt state not installed for store");
    };
    let state = store_state.get_mut();
    state.installations -= 1;
    if state.installations == 0 {
        store_state.remove();
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

    unsafe {
        #[allow(trivial_numeric_casts)]
        let errno = libc::pthread_kill(store_state.pthread as libc::pthread_t, libc::SIGUSR1);
        if errno != 0 {
            let _ = store_state
                .thread_current_signal_target_store
                .compare_exchange(
                    store_id.as_raw().get(),
                    0,
                    Ordering::Release,
                    Ordering::Relaxed,
                );
            let error_str = CStr::from_ptr(libc::strerror(errno)).to_str().unwrap();
            return Err(InterruptError::FailedToSendSignal(error_str));
        }
    }

    Ok(())
}

/// Acknowledges the signal and returns the store it was sent to interrupt.
pub(crate) fn take_signal_target() -> usize {
    SIGNAL_TARGET.with(|slot| {
        let target = slot.load(Ordering::Relaxed);
        // The owner on this thread clears the slot before releasing its Arc.
        unsafe { target.as_ref() }.map_or(0, |target| target.swap(0, Ordering::Acquire))
    })
}

/// Returns true if the store with the given ID has already been interrupted.
pub fn is_interrupted(store_id: StoreId) -> bool {
    STORE_INTERRUPT_STATE
        .get(&store_id)
        .is_some_and(|state| state.interrupted.load(Ordering::Acquire))
}

/// Returns the interruption flag for a registered store.
#[doc(hidden)]
pub fn interrupt_flag(store_id: StoreId) -> Option<Arc<AtomicBool>> {
    STORE_INTERRUPT_STATE
        .get(&store_id)
        .map(|state| Arc::clone(&state.interrupted))
}
