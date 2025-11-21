use std::{
    cell::RefCell,
    collections::HashMap,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    ptr,
    rc::Rc,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use corosensei::{Coroutine, CoroutineResult, Yielder};

use super::entities::function::Function as SysFunction;
use crate::{AsStoreMut, AsStoreRef, RuntimeError, Store, StoreContext, StoreMut, Value};
use wasmer_types::StoreId;

type HostFuture = Pin<Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>> + 'static>>;

pub(crate) fn call_function_async<'a>(
    function: SysFunction,
    store: Store,
    params: Vec<Value>,
) -> AsyncCallFuture<'a> {
    AsyncCallFuture::new(function, store, params)
}

struct AsyncYield(HostFuture);

enum AsyncResume {
    Start,
    HostFutureReady(Result<Vec<Value>, RuntimeError>),
}

pub(crate) struct AsyncCallFuture<'a> {
    coroutine: Option<Coroutine<AsyncResume, AsyncYield, Result<Box<[Value]>, RuntimeError>>>,
    pending_store_install: Option<Pin<Box<dyn Future<Output = StoreContextInstaller> + 'a>>>,
    pending_future: Option<HostFuture>,
    next_resume: Option<AsyncResume>,
    result: Option<Result<Box<[Value]>, RuntimeError>>,

    // Store handle we can use to lock the store down
    store: Store,

    // Use Rc<RefCell<...>> to make sure that the future is !Send and !Sync
    _marker: PhantomData<Rc<RefCell<&'a mut ()>>>,
}

// We can't use any of the existing AsStoreMut types here, since we keep
// changing the store context underneath us while the coroutine yields.
// To work around it, we use this dummy struct, which just grabs the store
// from the store context. Since we always have a store context installed
// when resuming the coroutine, this is safe in that it can access the store
// through the store context. HOWEVER, references returned from this struct
// CAN NOT BE HELD ACROSS A YIELD POINT. We don't do this anywhere in the
// `Function::call code.
struct AsyncCallStoreMut {
    store_id: StoreId,
}

impl AsStoreRef for AsyncCallStoreMut {
    fn as_ref(&self) -> &crate::StoreInner {
        // Safety: This is only used with Function::call, which doesn't store
        // the returned reference anywhere, including when calling into WASM
        // code.
        unsafe {
            StoreContext::get_current_transient(self.store_id)
                .as_ref()
                .unwrap()
                .as_ref()
        }
    }
}

impl AsStoreMut for AsyncCallStoreMut {
    fn as_mut(&mut self) -> &mut crate::StoreInner {
        // Safety: This is only used with Function::call, which doesn't store
        // the returned reference anywhere, including when calling into WASM
        // code.
        unsafe {
            StoreContext::get_current_transient(self.store_id)
                .as_mut()
                .unwrap()
                .as_mut()
        }
    }

    fn reborrow_mut(&mut self) -> &mut StoreMut {
        // Safety: This is only used with Function::call, which doesn't store
        // the returned reference anywhere, including when calling into WASM
        // code.
        unsafe {
            StoreContext::get_current_transient(self.store_id)
                .as_mut()
                .unwrap()
                .reborrow_mut()
        }
    }
}

impl<'a> AsyncCallFuture<'a> {
    pub(crate) fn new(function: SysFunction, store: crate::Store, params: Vec<Value>) -> Self {
        let store_id = store.id;
        let coroutine =
            Coroutine::new(move |yielder: &Yielder<AsyncResume, AsyncYield>, resume| {
                assert!(matches!(resume, AsyncResume::Start));

                let ctx_state = CoroutineContext::new(yielder);
                ctx_state.enter();
                let result = {
                    let mut store_mut = AsyncCallStoreMut { store_id };
                    function.call(&mut store_mut, &params)
                };
                ctx_state.leave();
                result
            });

        Self {
            coroutine: Some(coroutine),
            pending_store_install: None,
            pending_future: None,
            next_resume: Some(AsyncResume::Start),
            result: None,
            store,
            _marker: PhantomData,
        }
    }
}

impl Future for AsyncCallFuture<'_> {
    type Output = Result<Box<[Value]>, RuntimeError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
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
                return Poll::Ready(self.result.take().expect("polled after completion"));
            }

            // Start a store installation if not in progress already
            if let None = self.pending_store_install {
                self.pending_store_install =
                    Some(Box::pin(StoreContextInstaller::install(Store {
                        id: self.store.id,
                        inner: self.store.inner.clone(),
                    })));
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

            let resume_arg = self.next_resume.take().expect("no resume arg available");
            let coroutine = self.coroutine.as_mut().unwrap();
            match coroutine.resume(resume_arg) {
                CoroutineResult::Yield(AsyncYield(fut)) => {
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

enum StoreContextInstaller {
    FromThreadContext(crate::StoreMutWrapper),
    Installed(crate::ForcedStoreInstallGuard),
}

impl StoreContextInstaller {
    async fn install(store: Store) -> Self {
        if let Some(wrapper) = unsafe { crate::StoreContext::try_get_current(store.id) } {
            // If we're already in the scope of this store, we can just reuse it.
            StoreContextInstaller::FromThreadContext(wrapper)
        } else {
            // Otherwise, need to acquire a new StoreMut.
            let store_mut = store.make_mut_async().await;
            let guard = crate::StoreContext::force_install(store_mut);
            StoreContextInstaller::Installed(guard)
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
    CURRENT_CONTEXT.with(|cell| {
        match CoroutineContext::get_current() {
            None => {
                // If there is no async context or we haven't entered it,
                // we can still directly run a future that doesn't block
                // inline.
                // Note, there can be an async context without an active
                // coroutine in the following scenario:
                //   call_async -> wasm code -> imported function ->
                //   call (non-async) -> wasm_code -> imported async function
                run_immediate(future)
            }
            Some(context) => {
                let ctx_ref = unsafe { context.as_ref().expect("valid context pointer") };

                // Leave the coroutine context since we're yielding back to the
                // parent stack, and will be inactive until the future is ready.
                ctx_ref.leave();

                // Now we can yield back to the runtime while we wait
                let result = ctx_ref
                    .block_on_future(Box::pin(future))
                    .map_err(AsyncRuntimeError::RuntimeError);

                // Once the future is ready, we borrow again and restore the current
                // coroutine.
                ctx_ref.enter();

                result
            }
        }
    })
}

thread_local! {
    static CURRENT_CONTEXT: RefCell<Vec<*const CoroutineContext>> = const { RefCell::new(Vec::new()) };
}

struct CoroutineContext {
    yielder: *const Yielder<AsyncResume, AsyncYield>,
}

impl CoroutineContext {
    fn new(yielder: &Yielder<AsyncResume, AsyncYield>) -> Self {
        Self {
            yielder: yielder as *const _,
        }
    }

    fn enter(&self) {
        CURRENT_CONTEXT.with(|cell| {
            let mut borrow = cell.borrow_mut();

            // Push this coroutine on top of the active stack.
            borrow.push(self as *const _);
        })
    }

    // Note: we don't use a drop-style guard here on purpose; if a panic
    // happens while a coroutine is running, CURRENT_CONTEXT will be in
    // an inconsistent state. corosensei will unwind all coroutine stacks
    // anyway, and if we had a guard that would get dropped and try to
    // leave its context, it'd panic again at the assert_eq! below.
    fn leave(&self) {
        CURRENT_CONTEXT.with(|cell| {
            let mut borrow = cell.borrow_mut();

            // Pop this coroutine from the active stack.
            assert_eq!(
                borrow.pop(),
                Some(self as *const _),
                "Active coroutine stack corrupted"
            );
        });
    }

    fn get_current() -> Option<*const Self> {
        CURRENT_CONTEXT.with(|cell| cell.borrow().last().copied())
    }

    fn block_on_future(&self, future: HostFuture) -> Result<Vec<Value>, RuntimeError> {
        let yielder = unsafe { self.yielder.as_ref().expect("yielder pointer valid") };
        match yielder.suspend(AsyncYield(future)) {
            AsyncResume::HostFutureReady(result) => result,
            AsyncResume::Start => unreachable!("coroutine resumed without start"),
        }
    }
}

fn run_immediate(
    future: impl Future<Output = Result<Vec<Value>, RuntimeError>> + 'static,
) -> Result<Vec<Value>, AsyncRuntimeError> {
    fn noop_raw_waker() -> RawWaker {
        fn no_op(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            noop_raw_waker()
        }
        let vtable = &RawWakerVTable::new(clone, no_op, no_op, no_op);
        RawWaker::new(ptr::null(), vtable)
    }

    let mut future = Box::pin(future);
    let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
    let mut cx = Context::from_waker(&waker);
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(result) => result.map_err(AsyncRuntimeError::RuntimeError),
        Poll::Pending => Err(AsyncRuntimeError::YieldOutsideAsyncContext),
    }
}
