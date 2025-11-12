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
use crate::{AsStoreMut, RuntimeError, Value};

type HostFuture = Pin<Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>> + Send + 'static>>;

pub(crate) fn call_function_async<'a, S>(
    function: SysFunction,
    store: &'a mut S,
    params: Vec<Value>,
) -> AsyncCallFuture<'a, S>
where
    S: AsStoreMut + 'static,
{
    AsyncCallFuture::new(function, store, params)
}

enum AsyncYield {
    HostFuture(HostFuture),
}

enum AsyncResume {
    Start,
    HostFutureReady(Result<Vec<Value>, RuntimeError>),
}

pub(crate) struct AsyncCallFuture<'a, S: AsStoreMut + 'static> {
    coroutine: Option<Coroutine<AsyncResume, AsyncYield, Result<Box<[Value]>, RuntimeError>>>,
    pending_future: Option<HostFuture>,
    next_resume: Option<AsyncResume>,
    result: Option<Result<Box<[Value]>, RuntimeError>>,

    // Use Rc<RefCell<...>> to make sure that the future is !Send and !Sync
    _marker: PhantomData<Rc<RefCell<&'a mut S>>>,
}

impl<'a, S> AsyncCallFuture<'a, S>
where
    S: AsStoreMut + 'static,
{
    pub(crate) fn new(function: SysFunction, store: &'a mut S, params: Vec<Value>) -> Self {
        let store_ptr = store as *mut S;
        let coroutine =
            Coroutine::new(move |yielder: &Yielder<AsyncResume, AsyncYield>, _resume| {
                let ctx_state = CoroutineContext::new(yielder);
                ctx_state.enter();
                let result = {
                    let store_ref = unsafe { &mut *store_ptr };
                    function.call(store_ref, &params)
                };
                ctx_state.leave();
                result
            });

        Self {
            coroutine: Some(coroutine),
            pending_future: None,
            next_resume: Some(AsyncResume::Start),
            result: None,
            _marker: PhantomData,
        }
    }
}

impl<'a, S> Future for AsyncCallFuture<'a, S>
where
    S: AsStoreMut + 'static,
{
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

            let resume_arg = self.next_resume.take().unwrap_or(AsyncResume::Start);
            let coroutine = match self.coroutine.as_mut() {
                Some(coro) => coro,
                None => return Poll::Ready(self.result.take().expect("polled after completion")),
            };
            match coroutine.resume(resume_arg) {
                CoroutineResult::Yield(AsyncYield::HostFuture(fut)) => {
                    self.pending_future = Some(fut);
                }
                CoroutineResult::Return(result) => {
                    self.coroutine = None;
                    self.result = Some(result);
                }
            }
        }
    }
}

pub enum AsyncRuntimeError {
    YieldOutsideAsyncContext,
    RuntimeError(RuntimeError),
}

pub(crate) fn block_on_host_future<Fut>(future: Fut) -> Result<Vec<Value>, AsyncRuntimeError>
where
    Fut: Future<Output = Result<Vec<Value>, RuntimeError>> + Send + 'static,
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
        match yielder.suspend(AsyncYield::HostFuture(future)) {
            AsyncResume::HostFutureReady(result) => result,
            AsyncResume::Start => unreachable!("coroutine resumed without start"),
        }
    }
}

fn run_immediate(
    future: impl Future<Output = Result<Vec<Value>, RuntimeError>> + Send + 'static,
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
