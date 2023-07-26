#![allow(dead_code, unused)]
use std::{
    sync::{Arc, Condvar, Mutex},
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use futures::Future;

pub struct InlineWaker {
    lock: Mutex<()>,
    condvar: Condvar,
}
impl InlineWaker {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            lock: Mutex::new(()),
            condvar: Condvar::new(),
        })
    }

    fn wake_now(&self) {
        // Note: This guard should be there to prevent race conditions however in the
        // browser it causes a lock up - some strange browser issue. What I suspect
        // is that the Condvar::wait call is not releasing the mutex lock
        #[cfg(not(feature = "js"))]
        let _guard = self.lock.lock().unwrap();

        self.condvar.notify_all();
    }

    pub fn as_waker(self: &Arc<Self>) -> Waker {
        let s: *const Self = Arc::into_raw(Arc::clone(self));
        let raw_waker = RawWaker::new(s as *const (), &VTABLE);
        unsafe { Waker::from_raw(raw_waker) }
    }

    #[cfg(not(feature = "js"))]
    pub fn block_on<'a, A>(task: impl Future<Output = A> + 'a) -> A {
        futures::executor::block_on(task)
    }

    #[cfg(feature = "js")]
    pub fn block_on<'a, A>(task: impl Future<Output = A> + 'a) -> A {
        // Create the waker
        let inline_waker = Self::new();
        let waker = inline_waker.as_waker();
        let mut cx = Context::from_waker(&waker);

        // We loop waiting for the waker to be woken, then we poll again
        let mut task = Box::pin(task);
        loop {
            let lock = inline_waker.lock.lock().unwrap();
            match task.as_mut().poll(&mut cx) {
                Poll::Pending => {
                    inline_waker.condvar.wait(lock).ok();
                }
                Poll::Ready(ret) => {
                    return ret;
                }
            }
        }
    }
}

fn inline_waker_wake(s: &InlineWaker) {
    let waker_arc = unsafe { Arc::from_raw(s) };
    waker_arc.wake_now();
}

fn inline_waker_clone(s: &InlineWaker) -> RawWaker {
    let arc = unsafe { Arc::from_raw(s) };
    std::mem::forget(arc.clone());
    RawWaker::new(Arc::into_raw(arc) as *const (), &VTABLE)
}

const VTABLE: RawWakerVTable = unsafe {
    RawWakerVTable::new(
        |s| inline_waker_clone(&*(s as *const InlineWaker)), // clone
        |s| inline_waker_wake(&*(s as *const InlineWaker)),  // wake
        |s| (*(s as *const InlineWaker)).wake_now(), // wake by ref (don't decrease refcount)
        |s| drop(Arc::from_raw(s as *const InlineWaker)), // decrease refcount
    )
};
