use std::{
    cell::Cell,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    ptr,
    task::{Context, Poll},
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
    _marker: PhantomData<&'a mut S>,
}

impl<'a, S> AsyncCallFuture<'a, S>
where
    S: AsStoreMut + 'static,
{
    pub(crate) fn new(function: SysFunction, store: &'a mut S, params: Vec<Value>) -> Self {
        let store_ptr = store as *mut S;
        let coroutine =
            Coroutine::new(move |yielder: &Yielder<AsyncResume, AsyncYield>, _resume| {
                let ctx_state = AsyncContextState::new(yielder);
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

thread_local! {
    static CURRENT_CONTEXT: Cell<*const AsyncContextState> = const { Cell::new(ptr::null()) };
}

pub(crate) fn block_on_host_future<Fut>(future: Fut) -> Result<Vec<Value>, RuntimeError>
where
    Fut: Future<Output = Result<Vec<Value>, RuntimeError>> + Send + 'static,
{
    CURRENT_CONTEXT.with(|cell| {
        let ptr = cell.get();
        if ptr.is_null() {
            Err(RuntimeError::new(
                "async host functions can only be used inside `call_async`",
            ))
        } else {
            unsafe { (&*ptr).block_on_future(Box::pin(future)) }
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

struct AsyncContextState {
    yielder: *const Yielder<AsyncResume, AsyncYield>,
}

impl AsyncContextState {
    fn new(yielder: &Yielder<AsyncResume, AsyncYield>) -> Self {
        Self {
            yielder: yielder as *const _,
        }
    }

    fn enter(&self) -> AsyncContextGuard {
        CURRENT_CONTEXT.with(|cell| {
            let previous = cell.replace(self as *const _);
            AsyncContextGuard { previous }
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

struct AsyncContextGuard {
    previous: *const AsyncContextState,
}

impl Drop for AsyncContextGuard {
    fn drop(&mut self) {
        CURRENT_CONTEXT.with(|cell| {
            cell.set(self.previous);
        });
    }
}
