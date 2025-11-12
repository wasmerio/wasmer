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
                let _guard = ctx_state.enter();
                let result = {
                    let store_ref = unsafe { &mut *store_ptr };
                    function.call(store_ref, &params)
                };
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

struct AsyncContext {
    next: u64,
    active: Vec<u64>,
    coroutines: HashMap<u64, *const CoroutineContext>,
}

thread_local! {
    static CURRENT_CONTEXT: RefCell<Option<AsyncContext>> = const { RefCell::new(None) };
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
        let mut borrow = cell.borrow_mut();
        match borrow.as_ref().and_then(|ctx| ctx.active.last().copied()) {
            None => {
                // If there is no async context or we haven't entered it,
                // we can still directly run a future that doesn't block
                // inline.
                // Note, there can be an async context without an active
                // coroutine in the following scenario:
                //   call_async -> wasm code -> imported function ->
                //   call (non-async) -> wasm_code -> imported async function

                // We drop the borrow anyway to let the future start a new
                // async context if needed.
                drop(borrow);
                run_immediate(future)
            }
            Some(current) => {
                // If we have an active coroutine, get the context and yielder
                let context = borrow.as_mut().unwrap();
                let coro_context = *context
                    .coroutines
                    .get(&current)
                    .expect("Coroutine context was already destroyed");

                // Then pop ourselves from the active stack since we're not active anymore
                // and un-borrow the cell for others to use.
                assert_eq!(
                    context.active.pop(),
                    Some(current),
                    "Active coroutine stack corrupted"
                );
                drop(borrow);

                // Now we can yield back to the runtime while we wait
                let result = unsafe { &*coro_context }
                    .block_on_future(Box::pin(future))
                    .map_err(AsyncRuntimeError::RuntimeError);

                // Once the future is ready, we borrow again and restore the current
                // coroutine.
                let mut borrow = cell.borrow_mut();
                let context = borrow
                    .as_mut()
                    .expect("The async context was destroyed while a coroutine was pending");
                if !context.coroutines.contains_key(&current) {
                    panic!("The coroutine context was destroyed while a coroutine was pending");
                }
                context.active.push(current);

                result
            }
        }
    })
}

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

struct CoroutineContext {
    yielder: *const Yielder<AsyncResume, AsyncYield>,
}

impl CoroutineContext {
    fn new(yielder: &Yielder<AsyncResume, AsyncYield>) -> Self {
        Self {
            yielder: yielder as *const _,
        }
    }

    fn enter(&self) -> CoroutineContextGuard {
        CURRENT_CONTEXT.with(|cell| {
            let mut borrow = cell.borrow_mut();

            // If there is no context yet, create one.
            if borrow.is_none() {
                *borrow = Some(AsyncContext {
                    next: 0,
                    active: vec![],
                    coroutines: HashMap::new(),
                });
            }

            let context = borrow.as_mut().unwrap();

            // Assign an ID to this coroutine context.
            let id = context.next;
            context.next += 1;

            // Add this context to the map and push it on top of the active stack.
            context.coroutines.insert(id, self as *const _);
            context.active.push(id);
            CoroutineContextGuard { id }
        })
    }

    fn block_on_future(&self, future: HostFuture) -> Result<Vec<Value>, RuntimeError> {
        let yielder = unsafe { &*self.yielder };
        match yielder.suspend(AsyncYield::HostFuture(future)) {
            AsyncResume::HostFutureReady(result) => result,
            AsyncResume::Start => unreachable!("coroutine resumed without start"),
        }
    }
}

struct CoroutineContextGuard {
    id: u64,
}

impl Drop for CoroutineContextGuard {
    fn drop(&mut self) {
        CURRENT_CONTEXT.with(|cell| {
            let mut borrow = cell.borrow_mut();
            let context = borrow
                .as_mut()
                .expect("The async context was destroyed while a coroutine was active");
            context.coroutines.remove(&self.id);
            assert_eq!(
                context.active.pop(),
                Some(self.id),
                "Active coroutine stack corrupted"
            );
            if context.coroutines.is_empty() {
                // If there are no more coroutines, remove the context.
                *borrow = None;
            }
        });
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
