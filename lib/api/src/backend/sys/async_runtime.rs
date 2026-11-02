use std::{
    cell::RefCell,
    collections::HashMap,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    ptr,
    rc::Rc,
    sync::{LazyLock, atomic::AtomicU64},
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use corosensei::{Coroutine, CoroutineResult, Yielder};
use dashmap::DashMap;

use super::entities::function::Function as SysFunction;
use crate::{
    AsStoreAsync, AsStoreMut, AsStoreRef, ForcedStoreInstallGuard, LocalRwLockWriteGuard,
    RuntimeError, Store, StoreAsync, StoreContext, StoreContextId, StoreInner, StoreMut, StoreRef,
    SuspendedStoreContexts, Value,
};
use wasmer_types::StoreId;

type HostFuture = Pin<Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>> + 'static>>;

pub(crate) fn call_function_async(
    function: SysFunction,
    store: StoreAsync,
    params: Vec<Value>,
) -> AsyncCallFuture {
    AsyncCallFuture::new(function, store, params)
}

struct AsyncYield(HostFuture);

enum AsyncResume {
    Start,
    HostFutureReady(Result<Vec<Value>, RuntimeError>),
}

type AsyncCallFutureId = u64;

static NEXT_ASYNC_CALL_FUTURE_ID: AtomicU64 = AtomicU64::new(0);

static ASYNC_CALL_FUTURE_WAKERS: LazyLock<DashMap<StoreId, HashMap<AsyncCallFutureId, Waker>>> =
    LazyLock::new(DashMap::new);

#[allow(clippy::type_complexity)]
pub(crate) struct AsyncCallFuture {
    id: AsyncCallFutureId,

    coroutine: Option<Coroutine<AsyncResume, AsyncYield, Result<Box<[Value]>, RuntimeError>>>,
    suspended_stores: SuspendedStoreContexts,
    pending_store_install: Option<Pin<Box<dyn Future<Output = ForcedStoreInstallGuard>>>>,
    pending_future: Option<HostFuture>,
    next_resume: Option<AsyncResume>,
    result: Option<Result<Box<[Value]>, RuntimeError>>,

    // Store handle we can use to lock the store down
    store: StoreAsync,
    store_context_id: StoreContextId,
}

// We can't use any of the existing AsStoreMut types here, since we keep
// changing the store context underneath us while the coroutine yields.
// To work around it, we use this dummy struct, which just grabs the store
// from the store context. Since we always have a store context installed
// when resuming the coroutine, this is safe in that it can access the store
// through the store context. HOWEVER, references returned from this struct
// CAN NOT BE HELD ACROSS A YIELD POINT. We don't do this anywhere in the
// `Function::call` code.
struct AsyncCallStoreMut {
    store_id: StoreId,
}

impl AsStoreRef for AsyncCallStoreMut {
    fn as_store_ref(&self) -> StoreRef<'_> {
        // Safety: This is only used with Function::call, which doesn't store
        // the returned reference anywhere, including when calling into WASM
        // code.
        unsafe {
            StoreRef {
                inner: StoreContext::get_current_transient(self.store_id)
                    .as_ref()
                    .unwrap(),
            }
        }
    }
}

impl AsStoreMut for AsyncCallStoreMut {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        // Safety: This is only used with Function::call, which doesn't store
        // the returned reference anywhere, including when calling into WASM
        // code.
        unsafe {
            StoreMut {
                inner: StoreContext::get_current_transient(self.store_id)
                    .as_mut()
                    .unwrap(),
            }
        }
    }

    fn objects_mut(&mut self) -> &mut crate::StoreObjects {
        // Safety: This is only used with Function::call, which doesn't store
        // the returned reference anywhere, including when calling into WASM
        // code.
        unsafe {
            &mut StoreContext::get_current_transient(self.store_id)
                .as_mut()
                .unwrap()
                .objects
        }
    }
}

impl AsyncCallFuture {
    pub(crate) fn new(function: SysFunction, store: StoreAsync, params: Vec<Value>) -> Self {
        let store_id = store.id;
        let store_context_id = StoreContext::new_installation_id();
        let coroutine =
            Coroutine::new(move |yielder: &Yielder<AsyncResume, AsyncYield>, resume| {
                assert!(matches!(resume, AsyncResume::Start));

                let ctx_state = CoroutineContext::new(yielder, store_context_id);
                ctx_state.enter();
                let result = {
                    let mut store_mut = AsyncCallStoreMut { store_id };
                    function.call(&mut store_mut, &params)
                };
                ctx_state.leave();
                result
            });

        Self {
            id: NEXT_ASYNC_CALL_FUTURE_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
            coroutine: Some(coroutine),
            suspended_stores: SuspendedStoreContexts::default(),
            pending_store_install: None,
            pending_future: None,
            next_resume: Some(AsyncResume::Start),
            result: None,
            store,
            store_context_id,
        }
    }

    fn remove_from_wakers_list(&self) {
        let mut wakers_entry = match ASYNC_CALL_FUTURE_WAKERS.entry(self.store.store_id()) {
            dashmap::Entry::Occupied(o) => o,
            dashmap::Entry::Vacant(v) => return,
        };

        let mut waker_map_ref = wakers_entry.get_mut();
        waker_map_ref.remove(&self.id);
        if waker_map_ref.is_empty() {
            wakers_entry.remove();
        }
    }
}

impl Future for AsyncCallFuture {
    type Output = Result<Box<[Value]>, RuntimeError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        assert!(
            StoreContext::is_empty(),
            "Function::call_async futures cannot be polled recursively from within another imported function"
        );
        loop {
            let store_id = self.store.store_id();

            #[cfg(feature = "experimental-host-interrupt")]
            {
                if super::vm::interrupt_registry::is_interrupted(store_id) {
                    self.remove_from_wakers_list();
                    return Poll::Ready(Err(super::vm::Trap::lib(
                        super::vm::TrapCode::HostInterrupt,
                    )
                    .into()));
                }
            }

            if let Some(future) = self.pending_future.as_mut() {
                match future.as_mut().poll(cx) {
                    Poll::Ready(result) => {
                        self.pending_future = None;
                        self.next_resume = Some(AsyncResume::HostFutureReady(result));
                    }
                    Poll::Pending => return Poll::Pending,
                }
            }

            // If we're ready, return early
            if self.coroutine.is_none() {
                self.remove_from_wakers_list();
                return Poll::Ready(self.result.take().expect("polled after completion"));
            }

            {
                let mut wakers_entry = ASYNC_CALL_FUTURE_WAKERS.entry(store_id).or_default();
                wakers_entry.insert(self.id, cx.waker().clone());
            }

            // Start a store installation if not in progress already
            if self.pending_store_install.is_none() {
                self.pending_store_install = Some(Box::pin(install_store_context(
                    StoreAsync {
                        id: self.store.id,
                        inner: self.store.inner.clone(),
                    },
                    self.store_context_id,
                )));
            }

            // Acquiring a store lock should be the last step before resuming
            // the coroutine, to minimize the time we hold the lock.
            let store_context_guard = match self
                .pending_store_install
                .as_mut()
                .unwrap()
                .as_mut()
                .poll(cx)
            {
                Poll::Ready(guard) => {
                    self.pending_store_install = None;
                    guard
                }
                Poll::Pending => return Poll::Pending,
            };

            store_context_guard.restore_suffix(&mut self.suspended_stores);
            let resume_arg = self.next_resume.take().expect("no resume arg available");
            let coroutine = self.coroutine.as_mut().unwrap();
            let boundary = CoroutineBoundary::new();
            let result = coroutine.resume(resume_arg);
            boundary.returned();
            match result {
                CoroutineResult::Yield(AsyncYield(fut)) => {
                    store_context_guard.suspend_suffix(&mut self.suspended_stores);
                    self.pending_future = Some(fut);
                }
                CoroutineResult::Return(result) => {
                    self.coroutine = None;
                    self.result = Some(result);
                }
            }

            // Uninstall the store context to unlock the store after the coroutine
            // yields or returns.
            drop(store_context_guard);
        }
    }
}

impl Drop for AsyncCallFuture {
    fn drop(&mut self) {
        self.remove_from_wakers_list();
    }
}

async fn install_store_context(
    store: StoreAsync,
    installation: StoreContextId,
) -> ForcedStoreInstallGuard {
    match unsafe { crate::StoreContext::try_get_current_async(store.id) } {
        crate::GetStoreAsyncGuardResult::NotInstalled => {
            // We always need to acquire a new write lock on the store.
            let store_guard = store.inner.write().await;
            crate::StoreContext::install_async_with_id(store_guard, installation)
        }
        _ => {
            // If we're already in a store context, it is unsafe to reuse
            // the existing store ref since it'll also be accessible from
            // the imported function that tried to poll us, which is a
            // double mutable borrow.
            // Note to people who discover this code: this *would* be safe
            // if we had a separate variation of call_async that just
            // used the existing coroutine context instead of spawning a
            // new coroutine. However, the current call_async always spawns
            // a new coroutine, so we can't allow this; every coroutine
            // needs to own its write lock on the store to make sure there
            // are no overlapping mutable borrows. If this is something
            // you're interested in, feel free to open a GitHub issue outlining
            // your use-case.
            panic!(
                "Function::call_async futures cannot be polled recursively \
                from within another imported function. If you need to await \
                a recursive call_async, consider spawning the future into \
                your async runtime and awaiting the resulting task; \
                e.g. tokio::task::spawn(func.call_async(...)).await"
            );
        }
    }
}

pub enum AsyncRuntimeError {
    YieldOutsideAsyncContext,
    RuntimeError(RuntimeError),
}

pub(crate) fn block_on_host_future<Fut>(future: Fut) -> Result<Vec<Value>, AsyncRuntimeError>
where
    Fut: Future<Output = Result<Vec<Value>, RuntimeError>> + 'static,
{
    match CoroutineContext::get_current() {
        None => {
            // If there is no async context or we haven't entered it,
            // we can still directly run a future that doesn't block
            // inline.
            run_immediate(future)
        }
        Some(context) => unsafe { context.as_ref().expect("valid context pointer") }
            .block_on_future(Box::pin(future))
            .map_err(AsyncRuntimeError::RuntimeError),
    }
}

// This function will only *wake* pending futures belonging to the given store.
// It is expected that the interrupt happens before this function is called, so
// that the futures can catch and react to `interrupt_registry::is_interrupted`
// correctly.
pub(crate) fn notify_pending_futures_of_interrupt(store_id: StoreId) {
    let dashmap::Entry::Occupied(entry) = ASYNC_CALL_FUTURE_WAKERS.entry(store_id) else {
        return;
    };

    for waker in entry.get().values() {
        waker.wake_by_ref();
    }
}

thread_local! {
    static CURRENT_CONTEXT: RefCell<Vec<*const CoroutineContext>> = const { RefCell::new(Vec::new()) };
}

struct CoroutineContext {
    yielder: *const Yielder<AsyncResume, AsyncYield>,
    store_context_id: StoreContextId,
}

/// Raw context pointers belong to coroutine stacks. Unwinding a resume destroys
/// those stacks before propagating the panic, so remove only that resume's suffix.
struct CoroutineBoundary {
    prefix: Vec<*const CoroutineContext>,
    returned: bool,
}

impl CoroutineBoundary {
    fn new() -> Self {
        Self {
            prefix: CURRENT_CONTEXT.with(|current| current.borrow().clone()),
            returned: false,
        }
    }

    fn returned(mut self) {
        CURRENT_CONTEXT.with(|current| {
            assert_eq!(
                *current.borrow(),
                self.prefix,
                "coroutine did not restore its context before yielding or returning"
            );
        });
        self.returned = true;
    }
}

impl Drop for CoroutineBoundary {
    fn drop(&mut self) {
        if !self.returned {
            CURRENT_CONTEXT.with(|current| {
                let mut contexts = current.borrow_mut();
                // Never discard a caller whose ancestry has been disturbed by
                // another invariant failure during unwinding.
                if contexts.starts_with(&self.prefix) {
                    contexts.truncate(self.prefix.len());
                }
            });
        }
    }
}

impl CoroutineContext {
    fn new(yielder: &Yielder<AsyncResume, AsyncYield>, store_context_id: StoreContextId) -> Self {
        Self {
            yielder: yielder as *const _,
            store_context_id,
        }
    }

    fn enter(&self) {
        CURRENT_CONTEXT.with(|cell| {
            let mut borrow = cell.borrow_mut();

            // Push this coroutine on top of the active stack.
            borrow.push(self as *const _);
        })
    }

    // The owning resume boundary handles panic cleanup. A stack-local guard
    // must not pop this context again while corosensei unwinds its stack.
    fn leave(&self) {
        CURRENT_CONTEXT.with(|cell| {
            let mut borrow = cell.borrow_mut();

            // Pop this coroutine from the active stack.
            assert_eq!(
                borrow.last().copied(),
                Some(self as *const _),
                "Active coroutine stack corrupted"
            );
            borrow.pop();
        });
    }

    fn get_current() -> Option<*const Self> {
        CURRENT_CONTEXT.with(|cell| cell.borrow().last().copied())
    }

    fn block_on_future(&self, future: HostFuture) -> Result<Vec<Value>, RuntimeError> {
        if !StoreContext::can_suspend_suffix(self.store_context_id) {
            return run_immediate(future).map_err(|error| match error {
                AsyncRuntimeError::RuntimeError(error) => error,
                AsyncRuntimeError::YieldOutsideAsyncContext => RuntimeError::new(
                    "cannot suspend a native call while a borrowed or incompatible Store context remains installed",
                ),
            });
        }
        // Leave the coroutine context since we're yielding back to the
        // parent stack, and will be inactive until the future is ready.
        self.leave();

        let yielder = unsafe { self.yielder.as_ref().expect("yielder pointer valid") };
        let result = match yielder.suspend(AsyncYield(future)) {
            AsyncResume::HostFutureReady(result) => result,
            AsyncResume::Start => unreachable!("coroutine resumed without start"),
        };

        // Once the future is ready, we restore the current coroutine
        // context.
        self.enter();

        result
    }
}

fn run_immediate(
    future: impl Future<Output = Result<Vec<Value>, RuntimeError>> + 'static,
) -> Result<Vec<Value>, AsyncRuntimeError> {
    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(result) => result.map_err(AsyncRuntimeError::RuntimeError),
        Poll::Pending => Err(AsyncRuntimeError::YieldOutsideAsyncContext),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Function, Instance, Module};

    #[test]
    fn a_distinct_store_async_call_rejects_recursive_poll_without_starting() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let entered = Arc::new(AtomicUsize::new(0));
        let mut store = Store::default();
        let count = entered.clone();
        let function = Function::new_typed(&mut store, move || {
            count.fetch_add(1, Ordering::SeqCst);
        });
        let store = store.into_async();
        let mut call = Box::pin(function.call_async(&store, vec![]));
        let other = Store::default().into_async();
        let root = StoreContext::install_async_with_id(
            other.inner.try_write().unwrap(),
            StoreContext::new_installation_id(),
        );
        let waker = futures::task::noop_waker();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                call.as_mut().poll(&mut Context::from_waker(&waker))
            }))
            .is_err()
        );
        assert_eq!(entered.load(Ordering::SeqCst), 0);
        assert_eq!(
            unsafe { StoreContext::get_current(other.id) }
                .as_ref()
                .objects()
                .id(),
            other.id
        );
        drop(root);
        // Rejection did not poll or poison the exact backend invocation.
        assert!(futures::executor::block_on(call).unwrap().is_empty());
        assert_eq!(entered.load(Ordering::SeqCst), 1);
        assert!(StoreContext::is_empty());
    }

    #[test]
    fn a_panicked_engine_coroutine_restores_context_before_the_next_call() {
        let mut store = Store::default();
        let panic = Function::new_typed(&mut store, || -> () {
            panic!("synchronous callback panic");
        });
        let ready = Function::new_async(&mut store, crate::FunctionType::new([], []), |_| async {
            Ok(vec![])
        });
        let module = Module::new(
            &store,
            r#"(module
            (import "proof" "panic" (func $panic))
            (import "proof" "ready" (func $ready))
            (func (export "bad") call $panic)
            (func (export "next") (result i32) call $ready i32.const 7))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &crate::imports! { "proof" => { "panic" => panic, "ready" => ready } },
        )
        .unwrap();
        let store = store.into_async();
        let bad = instance.exports.get_function("bad").unwrap();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                futures::executor::block_on(bad.call_async(&store, vec![]))
            }))
            .is_err()
        );
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
        assert!(StoreContext::is_empty());
        let next = instance.exports.get_function("next").unwrap();
        let result = futures::executor::block_on(next.call_async(&store, vec![])).unwrap();
        assert_eq!(result[0].i32(), Some(7));
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
        assert!(StoreContext::is_empty());
    }

    #[test]
    fn a_panicked_nested_coroutine_preserves_its_live_parent_context() {
        let mut parent = Coroutine::new(
            |yielder: &Yielder<AsyncResume, AsyncYield>, _: AsyncResume| {
                let context = CoroutineContext::new(yielder, StoreContext::new_installation_id());
                context.enter();
                let mut child = Coroutine::new(
                    |yielder: &Yielder<AsyncResume, AsyncYield>,
                     _: AsyncResume|
                     -> Result<Box<[Value]>, RuntimeError> {
                        let context =
                            CoroutineContext::new(yielder, StoreContext::new_installation_id());
                        context.enter();
                        panic!("nested coroutine panic");
                    },
                );
                let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let boundary = CoroutineBoundary::new();
                    let _ = child.resume(AsyncResume::Start);
                    boundary.returned();
                }));
                let retained =
                    CURRENT_CONTEXT.with(|current| *current.borrow() == vec![&context as *const _]);
                context.leave();
                assert!(caught.is_err());
                assert!(retained);
                Ok::<_, RuntimeError>(Vec::<Value>::new().into_boxed_slice())
            },
        );
        let boundary = CoroutineBoundary::new();
        let result = parent.resume(AsyncResume::Start);
        boundary.returned();
        assert!(matches!(result, CoroutineResult::Return(Ok(_))));
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
    }
}
