use std::{
    sync::{mpsc, Arc},
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use futures::Future;

pub struct InlineWaker {
    waker_rx: mpsc::Receiver<()>,
    waker_tx: mpsc::Sender<()>,
}
impl InlineWaker {
    pub fn new() -> Arc<Self> {
        let (tx, rx) = mpsc::channel();
        Arc::new(Self {
            waker_rx: rx,
            waker_tx: tx,
        })
    }

    fn wake_now(&self) {
        self.waker_tx.send(()).ok();
    }

    pub fn as_waker(self: &Arc<InlineWaker>) -> Waker {
        let s: *const InlineWaker = Arc::into_raw(Arc::clone(self));
        let raw_waker = RawWaker::new(s as *const (), &VTABLE);
        unsafe { Waker::from_raw(raw_waker) }
    }

    pub fn block_on<'a, A>(task: impl Future<Output = A> + 'a) -> A {
        // Create the waker
        let inline_waker = Self::new();
        let waker = inline_waker.as_waker();
        let mut cx = Context::from_waker(&waker);

        // We loop waiting for the waker to be woken, then we poll again
        let mut task = Box::pin(task);
        loop {
            match task.as_mut().poll(&mut cx) {
                Poll::Pending => inline_waker.waker_rx.recv().expect(
                    "It should not be possible by design for the waker to close in this loop",
                ),
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
