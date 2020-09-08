//! Wasio core executor.

use crate::sys;
use crate::types::*;
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::rc::Rc;

pub struct Continuation {
    result: Option<__wasi_errno_t>,
    waker: Option<Waker>,
}

pub struct IoFuture<F> {
    continuation: Rc<RefCell<Continuation>>,
    token: Option<CancellationToken>,
    trigger: Option<F>,
}

impl<F: FnOnce(UserContext) -> Result<CancellationToken, __wasi_errno_t>> IoFuture<F> {
    pub(crate) fn new(trigger: F) -> Self {
        IoFuture {
            continuation: Rc::new(RefCell::new(Continuation {
                result: None,
                waker: None,
            })),
            token: None,
            trigger: Some(trigger),
        }
    }
}

impl<F: FnOnce(UserContext) -> Result<CancellationToken, __wasi_errno_t> + Unpin> Future
    for IoFuture<F>
{
    type Output = __wasi_errno_t;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let result = self.continuation.borrow().result;
        match result {
            Some(x) => Poll::Ready(x),
            None => {
                if self.token.is_none() {
                    self.continuation.borrow_mut().waker = Some(cx.waker().clone());
                    let trigger = self.as_mut().trigger.take().unwrap();
                    let ret = trigger(UserContext(Rc::into_raw(self.continuation.clone()) as u64));
                    match ret {
                        Ok(ct) => {
                            self.token = Some(ct);
                        }
                        Err(e) => {
                            self.continuation.borrow_mut().result = Some(e);
                        }
                    }
                }
                Poll::Pending
            }
        }
    }
}

impl<F> Drop for IoFuture<F> {
    fn drop(&mut self) {
        if self.continuation.borrow().result.is_none() {
            if let Some(token) = self.token.take() {
                unsafe {
                    sys::cancel(token);
                }
            }
        }
    }
}

/// Enters the event loop once.
pub fn enter_once() {
    let mut err: __wasi_errno_t = 0;
    let mut continuation: Option<Rc<RefCell<Continuation>>> = None;
    unsafe {
        assert_eq!(
            sys::wait(&mut err, &mut continuation as *mut _ as *mut UserContext),
            0
        );
    }
    println!("enter_once received err = {}, continuation = {:p}", err, &**continuation.as_ref().unwrap());
    let continuation = continuation.unwrap();

    let mut continuation = continuation.borrow_mut();
    continuation.result = Some(err);
    let waker = continuation.waker.take().unwrap();
    drop(continuation); // drop the borrow handle

    waker.wake();
}

/// Enters the event loop.
pub fn enter() -> ! {
    loop {
        enter_once();
    }
}
