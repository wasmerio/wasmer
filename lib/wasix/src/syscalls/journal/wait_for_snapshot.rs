use super::*;

#[cfg(not(feature = "journal"))]
pub fn wait_for_snapshot(_env: &WasiEnv) -> Pin<Box<dyn Future<Output = ()> + Send + Sync>> {
    Box::pin(std::future::pending())
}

#[cfg(feature = "journal")]
pub fn wait_for_snapshot(env: &WasiEnv) -> Pin<Box<dyn Future<Output = ()> + Send + Sync>> {
    use crate::os::task::process::{LockableWasiProcessInner, WasiProcessCheckpoint};

    struct Poller {
        inner: LockableWasiProcessInner,
    }
    impl Future for Poller {
        type Output = ();
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let mut guard = self.inner.0.lock().unwrap();
            if !matches!(guard.checkpoint, WasiProcessCheckpoint::Execute) {
                return Poll::Ready(());
            }
            if !guard.wakers.iter().any(|w| w.will_wake(cx.waker())) {
                guard.wakers.push(cx.waker().clone());
            }
            Poll::Pending
        }
    }
    Box::pin(Poller {
        inner: env.process.inner.clone(),
    })
}
