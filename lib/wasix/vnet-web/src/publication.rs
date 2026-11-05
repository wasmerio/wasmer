use std::{
    net::SocketAddr,
    sync::Mutex,
    task::{Poll, Waker},
};
use wasixv2_abi::Errno;

#[derive(Default)]
struct State {
    result: Option<Result<SocketAddr, Errno>>,
    waker: Option<Waker>,
}
#[derive(Default)]
pub(crate) struct Binding {
    state: Mutex<State>,
}
impl Binding {
    pub async fn wait(&self) -> Result<SocketAddr, Errno> {
        futures_util::future::poll_fn(|cx| {
            let mut state = self.state.lock().unwrap();
            match state.result {
                Some(result) => Poll::Ready(result),
                None => {
                    state.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        })
        .await
    }
    /// The caller wakes the returned waker AFTER releasing the backend lock.
    pub fn complete(&self, result: Result<SocketAddr, Errno>) -> Option<Waker> {
        let mut state = self.state.lock().unwrap();
        if state.result.is_none() {
            state.result = Some(result);
        }
        state.waker.take()
    }
}
