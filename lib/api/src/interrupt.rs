//! Opt-in host interruption for native WebAssembly calls.

use std::{
    cell::UnsafeCell,
    io,
    num::NonZeroUsize,
    panic::{self, AssertUnwindSafe},
    ptr,
    sync::{
        Arc, LazyLock, Once,
        atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering},
    },
};

use dashmap::{DashMap, Entry};
use wasmer_types::StoreId;
use wasmer_vm::{
    Trap, TrapCode, TrapHandlerFn, VMConfig, VMFunctionBody, VMFunctionContext, VMTrampoline,
    on_host_stack, raise_lib_trap, resume_panic, wasmer_call_trampoline,
};

use crate::backend::sys::entities::function::InvocationResult;
use crate::{AsStoreRef, CallWrapper, RuntimeError, WasmCall};

/// A handle for interrupting WebAssembly execution on a native store.
#[derive(Clone)]
pub struct Interrupter {
    store_id: StoreId,
}

impl Interrupter {
    /// Builds an interrupt handle for a native store ID.
    pub fn new(store_id: StoreId) -> Self {
        Self { store_id }
    }

    /// Requests an interrupt for the owning store.
    pub fn interrupt(&self) {
        if !matches!(
            interrupt(self.store_id),
            Err(InterruptError::StoreNotRunning)
        ) {
            #[cfg(feature = "experimental-async")]
            crate::backend::sys::async_runtime::notify_pending_futures_of_interrupt(self.store_id);
        }
    }
}

/// Enables host interruption for a synchronous native function call.
///
/// Pass this wrapper to [`Function::call_wrapped`](crate::Function::call_wrapped)
/// or [`TypedFunction::call_wrapped`](crate::TypedFunction::call_wrapped).
/// Use a clone of the interrupter to request interruption.
pub struct InterruptCallWrapper<W = fn(&mut WasmCall<'_>) -> Result<(), RuntimeError>> {
    interrupter: Interrupter,
    inner: W,
}

impl InterruptCallWrapper {
    /// Creates a wrapper for calls into the interrupter's store.
    #[inline]
    pub fn new(interrupter: Interrupter) -> Self {
        Self {
            interrupter,
            inner: |call| call.invoke(),
        }
    }

    /// Runs another wrapper with interruption enabled for each of its invocations.
    pub fn with_wrapper<T: CallWrapper>(self, inner: T) -> InterruptCallWrapper<T> {
        InterruptCallWrapper {
            interrupter: self.interrupter,
            inner,
        }
    }

    // Safety: call on the Wasm stack with no live destructors except `result`.
    #[inline]
    pub(crate) unsafe fn finish_host_invocation<T, E>(
        result: std::thread::Result<InvocationResult<T, E>>,
    ) -> std::thread::Result<InvocationResult<T, E>> {
        if matches!(
            result,
            Ok(InvocationResult::Success(_) | InvocationResult::Exception(_))
        ) && wasmer_vm::current_signal_token().is_some_and(is_token_interrupted)
        {
            let cleanup = on_host_stack(|| panic::catch_unwind(AssertUnwindSafe(|| drop(result))));
            // The host result has been dropped on the host stack.
            unsafe {
                match cleanup {
                    Ok(()) => raise_lib_trap(Trap::lib(TrapCode::HostInterrupt)),
                    Err(panic) => resume_panic(panic),
                }
            }
        }
        result
    }
}

impl<W: CallWrapper> CallWrapper for InterruptCallWrapper<W> {
    fn wrap(&mut self, call: &mut WasmCall<'_>) -> Result<(), RuntimeError> {
        if call.as_store_ref().objects().id() != self.interrupter.store_id {
            return Err(RuntimeError::new(
                "interrupt wrapper belongs to a different store",
            ));
        }
        self.inner.wrap(&mut call.interruptible())
    }
}

#[inline]
pub(crate) unsafe fn invoke_trampoline(
    trap_handler: Option<*const TrapHandlerFn<'static>>,
    config: &VMConfig,
    vmctx: VMFunctionContext,
    trampoline: VMTrampoline,
    callee: *const VMFunctionBody,
    values: *mut u8,
    interrupt_store: Option<StoreId>,
) -> Result<(), Trap> {
    if let Some(store_id) = interrupt_store {
        let _guard = install(store_id).map_err(|_| Trap::lib(TrapCode::HostInterrupt))?;
        unsafe {
            wasmer_vm::wasmer_call_trampoline_with_signal_token(
                trap_handler,
                config,
                vmctx,
                trampoline,
                callee,
                values,
                store_id.as_raw(),
            )
        }
    } else {
        unsafe { wasmer_call_trampoline(trap_handler, config, vmctx, trampoline, callee, values) }
    }
}

fn init_signal_handler() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        if cfg!(miri) {
            return;
        }
        unsafe {
            let mut handler: libc::sigaction = std::mem::zeroed();
            handler.sa_sigaction = signal_handler as *const () as usize;
            handler.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
            libc::sigemptyset(&mut handler.sa_mask);
            if libc::sigaction(libc::SIGUSR1, &handler, std::ptr::null_mut()) != 0 {
                panic!(
                    "unable to install host interrupt handler: {}",
                    io::Error::last_os_error()
                );
            }
        }
    });
}

unsafe extern "C" fn signal_handler(
    signal: libc::c_int,
    siginfo: *mut libc::siginfo_t,
    context: *mut libc::c_void,
) {
    if let Some(target) = NonZeroUsize::new(take_signal_target())
        && wasmer_vm::current_signal_token() == Some(target)
    {
        unsafe {
            wasmer_vm::try_raise_signal_trap(signal, siginfo, context, TrapCode::HostInterrupt)
        };
    }
}

#[derive(Debug, thiserror::Error)]
#[error("This store was already interrupted and can't be entered again")]
struct InstallError;

#[derive(Debug, thiserror::Error)]
enum InterruptError {
    #[error("Store not running")]
    StoreNotRunning,
    #[error("Another interrupt is already in progress on the target thread")]
    OtherInterruptInProgress,
    #[error("Failed to send interrupt signal: {0}")]
    FailedToSendSignal(io::Error),
}

struct InterruptInstallGuard {
    store_id: StoreId,
}

impl Drop for InterruptInstallGuard {
    fn drop(&mut self) {
        uninstall(self.store_id);
    }
}

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
    signal_target: Arc<AtomicUsize>,
}

static STORE_INTERRUPT_STATE: LazyLock<DashMap<StoreId, StoreInterruptState>> =
    LazyLock::new(Default::default);

thread_local! {
    // Mutation happens on the host stack. Reading on the Wasm stack must
    // avoid a borrow guard because a signal can abandon that stack.
    static ACTIVE_STORES: UnsafeCell<Vec<(StoreId, Arc<AtomicBool>)>> = const { UnsafeCell::new(Vec::new()) };
    // Keep handler access allocation-free even on a thread without a registration.
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

/// Install interrupt state for the given store. Note that this function
/// may be called more than once, and correctly maintains a stack of
/// stores for which the state is installed.
fn install(store_id: StoreId) -> Result<InterruptInstallGuard, InstallError> {
    init_signal_handler();
    let store_state = STORE_INTERRUPT_STATE.entry(store_id).or_insert_with(|| {
        let signal_target = SIGNAL_TARGET_OWNER.with(|target| Arc::clone(&target.0));

        #[allow(trivial_numeric_casts)]
        let pthread = unsafe { libc::pthread_self() as usize };

        StoreInterruptState {
            pthread,
            interrupted: Arc::new(AtomicBool::new(false)),
            signal_target,
        }
    });

    if store_state.interrupted.load(Ordering::Acquire) {
        return Err(InstallError);
    }

    ACTIVE_STORES.with(|stores| {
        // The signal handler does not access the registration list.
        let stores = unsafe { &mut *stores.get() };
        stores.push((store_id, Arc::clone(&store_state.interrupted)));
    });

    Ok(InterruptInstallGuard { store_id })
}

fn uninstall(store_id: StoreId) {
    let Entry::Occupied(store_state_entry) = STORE_INTERRUPT_STATE.entry(store_id) else {
        panic!("Internal error: interrupt state not installed for store");
    };

    let has_more_installations = ACTIVE_STORES.with(|stores| {
        // The signal handler does not access the registration list.
        let stores = unsafe { &mut *stores.get() };
        assert!(
            stores.pop_if(|(id, _)| *id == store_id).is_some(),
            "InterruptInstallGuard dropped out of order"
        );
        stores.iter().any(|(id, _)| *id == store_id)
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
fn interrupt(store_id: StoreId) -> Result<(), InterruptError> {
    let Entry::Occupied(mut store_state) = STORE_INTERRUPT_STATE.entry(store_id) else {
        return Err(InterruptError::StoreNotRunning);
    };
    let store_state = store_state.get_mut();

    if store_state
        .signal_target
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
            let _ = store_state.signal_target.compare_exchange(
                store_id.as_raw().get(),
                0,
                Ordering::Release,
                Ordering::Relaxed,
            );
            return Err(InterruptError::FailedToSendSignal(
                io::Error::from_raw_os_error(errno),
            ));
        }
    }

    Ok(())
}

// The thread-local owner clears this pointer before dropping its Arc.
fn take_signal_target() -> usize {
    SIGNAL_TARGET.with(|slot| {
        let target = slot.load(Ordering::Relaxed);
        unsafe { target.as_ref() }.map_or(0, |target| target.swap(0, Ordering::Acquire))
    })
}

/// Returns true if the store with the given ID is installed on this thread
/// and has already been interrupted.
#[cfg(feature = "experimental-async")]
#[inline]
pub(crate) fn is_interrupted(store_id: StoreId) -> bool {
    is_token_interrupted(store_id.as_raw())
}

#[inline]
fn is_token_interrupted(token: NonZeroUsize) -> bool {
    ACTIVE_STORES.with(|stores| {
        // This read cannot call back into the registry.
        let stores = unsafe { &*stores.get() };
        stores
            .iter()
            .rev()
            .find(|(id, _)| id.as_raw() == token)
            .is_some_and(|(_, interrupted)| interrupted.load(Ordering::Acquire))
    })
}

#[cfg(test)]
#[cfg(not(miri))]
mod tests {
    use super::*;

    #[test]
    fn interrupted_store_cant_be_entered_again() -> anyhow::Result<()> {
        let store_id = StoreId::default();
        let _guard = install(store_id)?;
        interrupt(store_id)?;
        assert!(matches!(install(store_id), Err(InstallError)));
        Ok(())
    }
}
