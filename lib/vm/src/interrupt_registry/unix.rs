use std::{
    cell::RefCell,
    ffi::CStr,
    marker::PhantomData,
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
    ///
    /// We store this as a plain integer because `libc::pthread_t` is a raw
    /// pointer on some Unix targets, which would make the global `DashMap`
    /// fail its `Send` bounds even though we only treat the value as an opaque
    /// thread identifier.
    pthread: usize,
    /// Whether this store was interrupted.
    interrupted: bool,
    /// Includes installations parked on suspended coroutine stacks.
    installations: usize,
    /// See comments in [`ThreadInterruptState`].
    thread_current_signal_target_store: Arc<AtomicUsize>,
}

/// Thread-related state; only **PARTS** of this struct are safe to access
/// from within the interrupt handler.
struct ThreadInterruptState {
    /// The handler never borrows this bookkeeping. Its atomics are separate
    /// fields, so a signal cannot alias an exclusive borrow of the whole state.
    bookkeeping: RefCell<Installations>,

    /// Always stores the top active Store ID. Needed since a vec is not
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct InstallationId(u64, PhantomData<*mut ()>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Installation {
    id: InstallationId,
    store_id: StoreId,
}

#[derive(Default)]
struct Installations {
    next_id: u64,
    active: Vec<Installation>,
    // A parked guard remains installed even while absent from `active`.
    installed: Vec<Installation>,
}

thread_local! {
    static THREAD_INTERRUPT_STATE: ThreadInterruptState = ThreadInterruptState {
        bookkeeping: RefCell::new(Installations::default()),
        current_active_store: AtomicUsize::new(0),
        current_signal_target_store: Arc::new(AtomicUsize::new(0)),
    };
}

/// Physical interrupt installations belonging to one suspended execution stack.
/// This does not own the guards: dropping a detached guard removes its exact ID.
#[doc(hidden)]
#[derive(Default)]
pub struct SavedInterruptState {
    active: Vec<Installation>,
    _thread: PhantomData<*mut ()>,
}

impl SavedInterruptState {
    /// Exchange the active physical stack, retaining all parked registrations.
    ///
    /// # Safety
    /// The guarded native stacks must remain on this thread, and the returned
    /// caller state must be restored before resuming that caller. No guest code
    /// may run between exchanging its VM trap state and interrupt state.
    pub unsafe fn replace_current(mut self) -> Self {
        THREAD_INTERRUPT_STATE.with(|state| {
            let mut bookkeeping = state.bookkeeping.borrow_mut();
            // Detached guards can be dropped while another root is active.
            // Never reinstall their old IDs, even for the same Store address.
            self.active
                .retain(|entry| bookkeeping.installed.contains(entry));
            std::mem::swap(&mut bookkeeping.active, &mut self.active);
            state.current_active_store.store(
                bookkeeping
                    .active
                    .last()
                    .map_or(0, |entry| entry.store_id.as_raw().get()),
                Ordering::Release,
            );
        });
        self
    }
}

/// Install interrupt state for the given store. Note that this function
/// may be called more than once, and correctly maintains a stack of
/// stores for which the state is installed.
pub fn install(store_id: StoreId) -> Result<InterruptInstallGuard, InstallError> {
    let installation = THREAD_INTERRUPT_STATE.with(|state| {
        let mut bookkeeping = state.bookkeeping.borrow_mut();
        bookkeeping.next_id = bookkeeping
            .next_id
            .checked_add(1)
            .expect("interrupt installation ID overflow");
        Installation {
            id: InstallationId(bookkeeping.next_id, PhantomData),
            store_id,
        }
    });
    let mut store_state = STORE_INTERRUPT_STATE.entry(store_id).or_insert_with(|| {
        let thread_current_signal_target_store =
            THREAD_INTERRUPT_STATE.with(|state| state.current_signal_target_store.clone());

        // Stores cannot move threads while one of their installations is alive.
        #[allow(trivial_numeric_casts)]
        let pthread = unsafe { libc::pthread_self() as usize };

        StoreInterruptState {
            pthread,
            interrupted: false,
            installations: 0,
            thread_current_signal_target_store,
        }
    });

    if store_state.interrupted {
        return Err(InstallError::AlreadyInterrupted);
    }
    let count = store_state
        .installations
        .checked_add(1)
        .expect("interrupt installation count overflow");
    THREAD_INTERRUPT_STATE.with(|state| {
        let mut bookkeeping = state.bookkeeping.borrow_mut();
        bookkeeping.installed.push(installation);
        bookkeeping.active.push(installation);
        state
            .current_active_store
            .store(store_id.as_raw().get(), Ordering::Release);
    });
    store_state.installations = count;

    Ok(InterruptInstallGuard {
        store_id,
        installation: installation.id,
    })
}

pub(super) fn uninstall(store_id: StoreId, installation: InstallationId) {
    let Entry::Occupied(mut store_state_entry) = STORE_INTERRUPT_STATE.entry(store_id) else {
        panic!("Internal error: interrupt state not installed for store");
    };

    THREAD_INTERRUPT_STATE.with(|state| {
        let mut bookkeeping = state.bookkeeping.borrow_mut();
        let installed = bookkeeping
            .installed
            .iter()
            .position(|entry| entry.id == installation && entry.store_id == store_id)
            .expect("interrupt installation not found");
        if let Some(active) = bookkeeping
            .active
            .iter()
            .position(|entry| entry.id == installation)
        {
            assert_eq!(
                active + 1,
                bookkeeping.active.len(),
                "InterruptInstallGuard dropped out of order"
            );
            bookkeeping.active.pop();
            state.current_active_store.store(
                bookkeeping
                    .active
                    .last()
                    .map_or(0, |entry| entry.store_id.as_raw().get()),
                Ordering::Release,
            );
        }
        // If detached, leave the foreign active prefix completely untouched.
        bookkeeping.installed.swap_remove(installed);
    });

    let count = store_state_entry
        .get()
        .installations
        .checked_sub(1)
        .expect("interrupt installation count underflow");
    if count == 0 {
        store_state_entry.remove();
    } else {
        store_state_entry.get_mut().installations = count;
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

    store_state.interrupted = true;

    unsafe {
        #[allow(trivial_numeric_casts)]
        let errno = libc::pthread_kill(store_state.pthread as libc::pthread_t, libc::SIGUSR1);
        if errno != 0 {
            let error_str = CStr::from_ptr(libc::strerror(errno)).to_str().unwrap();
            return Err(InterruptError::FailedToSendSignal(error_str));
        }
    }

    Ok(())
}

/// Called from within the signal handler to decide whether we should interrupt
/// the currently running WASM code. This function *MAY* return junk results in
/// case a signal comes in during an install, uninstall or physical stack
/// exchange. In those transitions no Wasm instruction is executing, and the
/// trap handler's stack-bound check prevents trapping the host stack.
pub(crate) fn on_interrupted() -> bool {
    THREAD_INTERRUPT_STATE.with(|t| {
        // Only these atomic fields are accessed from the signal handler.
        let state = t;

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
    let Entry::Occupied(store_state_entry) = STORE_INTERRUPT_STATE.entry(store_id) else {
        return false;
    };
    store_state_entry.get().interrupted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active_store() -> usize {
        THREAD_INTERRUPT_STATE.with(|state| state.current_active_store.load(Ordering::Acquire))
    }

    #[test]
    fn dropping_a_detached_guard_preserves_the_foreign_active_installation() {
        let first_id = StoreId::default();
        let second_id = StoreId::default();
        let first = install(first_id).unwrap();
        let parked_first = unsafe { SavedInterruptState::default().replace_current() };
        assert_eq!(active_store(), 0);
        let second = install(second_id).unwrap();
        drop(first);
        assert_eq!(active_store(), second_id.as_raw().get());
        assert!(!STORE_INTERRUPT_STATE.contains_key(&first_id));
        assert_eq!(
            STORE_INTERRUPT_STATE.get(&second_id).unwrap().installations,
            1
        );

        let parked_second = unsafe { parked_first.replace_current() };
        assert_eq!(active_store(), 0, "removed first ID must not be restored");
        drop(second);
        let empty = unsafe { parked_second.replace_current() };
        assert!(empty.active.is_empty());
        assert_eq!(active_store(), 0);
        assert!(!STORE_INTERRUPT_STATE.contains_key(&second_id));
    }

    #[test]
    fn the_same_store_keeps_its_parked_registration_until_the_last_guard_drops() {
        let id = StoreId::default();
        let first = install(id).unwrap();
        let parked_first = unsafe { SavedInterruptState::default().replace_current() };
        let second = install(id).unwrap();
        assert_ne!(first.installation, second.installation);
        assert_eq!(STORE_INTERRUPT_STATE.get(&id).unwrap().installations, 2);
        drop(first);
        assert_eq!(active_store(), id.as_raw().get());
        assert_eq!(STORE_INTERRUPT_STATE.get(&id).unwrap().installations, 1);
        drop(second);
        assert!(!STORE_INTERRUPT_STATE.contains_key(&id));
        let empty = unsafe { parked_first.replace_current() };
        assert!(empty.active.is_empty());
        assert_eq!(active_store(), 0);
    }

    #[test]
    fn an_active_guard_still_rejects_removal_below_another_installation() {
        let first = install(StoreId::default()).unwrap();
        let second = install(StoreId::default()).unwrap();
        // Call bookkeeping directly to retain both guards after the rejected
        // operation, then remove them in their original valid order.
        assert!(
            std::panic::catch_unwind(|| uninstall(first.store_id, first.installation)).is_err()
        );
        assert_eq!(active_store(), second.store_id.as_raw().get());
        drop(second);
        assert_eq!(active_store(), first.store_id.as_raw().get());
        drop(first);
        assert_eq!(active_store(), 0);
    }
}
