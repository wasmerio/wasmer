use std::task::{Context, Poll, Waker};

use futures::Future;

pub struct InlineWaker;

impl InlineWaker {
    pub fn block_on<'a, A>(task: impl Future<Output = A> + 'a) -> A {
        let (parker, unparker) = parking::pair();
        let waker = Waker::from(unparker.clone());
        let mut cx = Context::from_waker(&waker);

        let mut task = Box::pin(task);
        loop {
            match task.as_mut().poll(&mut cx) {
                Poll::Pending => {
                    parker.park();
                }
                Poll::Ready(ret) => {
                    return ret;
                }
            }
        }
    }
}
