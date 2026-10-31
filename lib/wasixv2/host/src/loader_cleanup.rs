//! Loader jobs only; unrelated detached host work does not delay runner retirement.

use crate::{Host, LocalFuture};
use std::{
    collections::VecDeque,
    future::{Future, poll_fn},
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, Weak},
    task::{Context, Poll, Waker},
};

/// A factory crosses threads; the local future is created only on the owning runner.
pub type LoaderCleanupJob = Box<dyn FnOnce() -> LocalFuture<'static, ()> + Send>;

/// Custody of loader release work for one guest runner. Ordinary Executor tasks stay detached.
pub struct LoaderCleanup {
    host: Arc<dyn Host>,
    state: Mutex<State>,
}
#[derive(Default)]
struct State {
    jobs: VecDeque<LoaderCleanupJob>,
    retire: Vec<Retirement>,
    pending: usize,
    closed: bool,
    waker: Option<Waker>,
}
struct Retirement {
    owner: Weak<()>,
    job: LoaderCleanupJob,
}
impl LoaderCleanup {
    pub fn new(host: Arc<dyn Host>) -> Arc<Self> {
        Arc::new(Self {
            host,
            state: Mutex::default(),
        })
    }
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub fn is_open(&self) -> bool {
        !self.state().closed
    }
    /// Enqueue a last-owner release on this runner, atomically with its final drain.
    /// A closed runner returns custody to the caller for another live attachment.
    pub fn submit(self: &Arc<Self>, job: LoaderCleanupJob) -> Result<(), LoaderCleanupJob> {
        let mut state = self.state();
        if state.closed {
            return Err(job);
        }
        state.pending += 1;
        state.jobs.push_back(job);
        let waker = state.waker.take();
        drop(state);
        if let Some(waker) = waker {
            waker.wake();
        }
        Ok(())
    }
    /// A weak loader attachment is checked after the guest/task teardown and all
    /// prior releases. The callback must not retain the image or store strongly.
    pub fn on_retire(
        &self,
        owner: &Arc<()>,
        job: LoaderCleanupJob,
    ) -> Result<(), LoaderCleanupJob> {
        let mut state = self.state();
        if state.closed {
            return Err(job);
        }
        // A runner spans exec replacements. Dead image callbacks must not grow
        // indefinitely while the next image keeps that runner alive.
        state
            .retire
            .retain(|retirement| retirement.owner.strong_count() != 0);
        state.retire.push(Retirement {
            owner: Arc::downgrade(owner),
            job,
        });
        Ok(())
    }
    /// Transfer an already suspended local cleanup operation. Call only from this
    /// runner's executor while its guest or another scoped job still owns custody.
    pub fn spawn(self: &Arc<Self>, future: LocalFuture<'static, ()>) {
        let mut state = self.state();
        if state.closed {
            drop(state);
            // Preserve the existing executor-context requirement for a trusted host
            // retaining ABI objects beyond their runner. Managed retirement never
            // takes this path; it cannot gain custody from an already ended runner.
            self.host.executor().spawn_local(future);
            return;
        }
        state.pending += 1;
        drop(state);
        self.start(future);
    }
    fn start(self: &Arc<Self>, future: LocalFuture<'static, ()>) {
        let completion = Completion(self.clone());
        self.host.executor().spawn_local(Box::pin(async move {
            future.await;
            drop(completion);
        }));
    }
    fn flush(self: &Arc<Self>, cx: &mut Context<'_>) {
        let jobs = {
            let mut state = self.state();
            state.waker = Some(cx.waker().clone());
            std::mem::take(&mut state.jobs)
        };
        for job in jobs {
            self.start(job());
        }
    }
    async fn drain(self: Arc<Self>) {
        poll_fn(|cx| {
            self.flush(cx);
            let retire = {
                let mut state = self.state();
                if state.pending != 0 {
                    return Poll::Pending;
                }
                let retire: Vec<_> = std::mem::take(&mut state.retire)
                    .into_iter()
                    .filter(|retirement| retirement.owner.strong_count() != 0)
                    .map(|retirement| retirement.job)
                    .collect();
                if retire.is_empty() {
                    state.closed = true;
                    state.waker = None;
                    return Poll::Ready(());
                }
                state.pending = retire.len();
                retire
            };
            for job in retire {
                self.start(job());
            }
            Poll::Pending
        })
        .await
    }
    /// Drive the guest and queued factories on this executor, then await only its
    /// loader jobs. kernel/exit.c:do_exit completes memory retirement before exit_notify.
    pub fn run<'a, R: 'a>(
        self: &Arc<Self>,
        guest: impl Future<Output = R> + 'a,
    ) -> impl Future<Output = R> + 'a {
        Runner {
            scope: self.clone(),
            guest: Some(Box::pin(guest)),
            output: None,
            drain: None,
            done: false,
        }
    }
}
struct Completion(Arc<LoaderCleanup>);
impl Drop for Completion {
    fn drop(&mut self) {
        let mut state = self.0.state();
        state.pending -= 1;
        let waker = state.waker.take();
        drop(state);
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}
struct Runner<'a, R> {
    scope: Arc<LoaderCleanup>,
    guest: Option<LocalFuture<'a, R>>,
    output: Option<R>,
    drain: Option<LocalFuture<'static, ()>>,
    done: bool,
}
// Only the boxed futures are pinned; output is never projected as pinned.
impl<R> Unpin for Runner<'_, R> {}
impl<R> Future for Runner<'_, R> {
    type Output = R;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<R> {
        let this = self.get_mut();
        this.scope.flush(cx);
        if let Some(guest) = &mut this.guest {
            match guest.as_mut().poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(output) => this.output = Some(output),
            }
            // Drop all frame/image attachments before starting the final drain.
            this.guest = None;
            this.drain = Some(Box::pin(this.scope.clone().drain()));
        }
        if this.drain.as_mut().unwrap().as_mut().poll(cx).is_pending() {
            return Poll::Pending;
        }
        this.drain = None;
        this.done = true;
        Poll::Ready(this.output.take().unwrap())
    }
}
impl<R> Drop for Runner<'_, R> {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        // Guest cancellation can enqueue more cleanup. Retain the exact draining
        // continuation, and never discard it when transferring runner custody.
        self.guest = None;
        {
            let mut state = self.scope.state();
            if state.pending == 0 && state.retire.is_empty() {
                state.closed = true;
                state.waker = None;
                return;
            }
        }
        let drain = self
            .drain
            .take()
            .unwrap_or_else(|| Box::pin(self.scope.clone().drain()));
        self.scope.host.executor().spawn_local(drain);
    }
}

#[cfg(all(test, feature = "tokio", not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::{Host, TokioHost};
    use std::{
        future::{Future, pending, poll_fn},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        task::Poll,
        time::Duration,
    };
    #[test]
    fn canceling_without_loader_work_needs_no_executor_context() {
        // Default/custom ABIs must not gain a detached task on cancellation.
        let host: Arc<dyn Host> = Arc::new(TokioHost::default());
        let scope = LoaderCleanup::new(host);
        drop(scope.run(pending::<()>()));
        assert!(!scope.is_open());
    }
    #[tokio::test]
    async fn runner_waits_for_loader_jobs_but_not_unrelated_host_work() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let host: Arc<dyn Host> = Arc::new(TokioHost::default());
                host.executor().spawn_local(Box::pin(pending()));
                let scope = LoaderCleanup::new(host);
                let done = Arc::new(AtomicBool::new(false));
                let job_done = done.clone();
                let result = scope
                    .run(async {
                        scope.spawn(Box::pin(async move {
                            tokio::time::sleep(Duration::from_millis(2)).await;
                            job_done.store(true, Ordering::Release);
                        }));
                        42
                    })
                    .await;
                assert_eq!(result, 42);
                assert!(done.load(Ordering::Acquire));
                assert!(!scope.is_open());
            })
            .await;
    }
    #[tokio::test]
    async fn final_weak_callbacks_follow_guest_teardown_and_prior_cleanup() {
        use std::sync::atomic::AtomicUsize;
        struct Guest(Arc<AtomicUsize>);
        impl Drop for Guest {
            fn drop(&mut self) {
                self.0.store(1, Ordering::Release);
            }
        }
        let local = tokio::task::LocalSet::new();
        let phase = Arc::new(AtomicUsize::new(0));
        let owner = Arc::new(());
        local
            .run_until(async {
                let scope = LoaderCleanup::new(Arc::new(TokioHost::default()));
                let image = Arc::new(());
                let weak = Arc::downgrade(&image);
                let checked = phase.clone();
                assert!(
                    scope
                        .on_retire(
                            &owner,
                            Box::new(move || Box::pin(async move {
                                assert!(
                                    weak.upgrade().is_none(),
                                    "weak callback must not pin image"
                                );
                                assert_eq!(checked.load(Ordering::Acquire), 2);
                                checked.store(3, Ordering::Release);
                            }))
                        )
                        .is_ok()
                );
                let stage = phase.clone();
                let mut runner = Box::pin(scope.run(async {
                    let _guest = Guest(phase.clone());
                    let _image = image;
                    scope.spawn(Box::pin(async move {
                        // The canceled guest drops before this executor polls its job.
                        assert_eq!(stage.load(Ordering::Acquire), 1);
                        stage.store(2, Ordering::Release);
                    }));
                    pending::<()>().await;
                }));
                poll_fn(|cx| {
                    assert!(runner.as_mut().poll(cx).is_pending());
                    Poll::Ready(())
                })
                .await;
                drop(runner);
            })
            .await;
        local.await;
        assert_eq!(phase.load(Ordering::Acquire), 3);
    }
    #[test]
    fn dead_weak_callbacks_stay_bounded_across_image_replacements() {
        let scope = LoaderCleanup::new(Arc::new(TokioHost::default()));
        for _ in 0..1000 {
            let owner = Arc::new(());
            assert!(
                scope
                    .on_retire(
                        &owner,
                        Box::new(|| Box::pin(async {
                            panic!("dead image callback must not execute");
                        }))
                    )
                    .is_ok()
            );
            assert!(scope.state().retire.len() <= 1);
        }
        let mut run = Box::pin(scope.run(async {}));
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(run.as_mut().poll(&mut cx).is_ready());
    }
    #[test]
    fn native_worker_finishes_scoped_release_before_retirement() {
        use std::sync::mpsc;
        let host: Arc<dyn Host> = Arc::new(TokioHost::default());
        let worker_host = host.clone();
        let (send, receive) = mpsc::channel();
        host.executor()
            .spawn_thread(Box::new(move || {
                let scope = LoaderCleanup::new(worker_host);
                Box::pin(async move {
                    let release = send.clone();
                    scope
                        .run(async {
                            scope.spawn(Box::pin(async move {
                                tokio::task::yield_now().await;
                                release.send("released").unwrap();
                            }));
                        })
                        .await;
                    send.send("retired").unwrap();
                })
            }))
            .unwrap();
        assert_eq!(
            receive.recv_timeout(Duration::from_secs(5)).unwrap(),
            "released"
        );
        assert_eq!(
            receive.recv_timeout(Duration::from_secs(5)).unwrap(),
            "retired"
        );
    }
    #[tokio::test]
    async fn canceling_runner_transfers_exact_drain_and_keeps_nonce() {
        let local = tokio::task::LocalSet::new();
        let weak = local
            .run_until(async {
                let host: Arc<dyn Host> = Arc::new(TokioHost::default());
                let scope = LoaderCleanup::new(host);
                let nonce = Arc::new(());
                let weak = Arc::downgrade(&nonce);
                let mut runner = Box::pin(scope.run(async {
                    scope.spawn(Box::pin(async move {
                        tokio::time::sleep(Duration::from_millis(2)).await;
                        drop(nonce);
                    }));
                }));
                poll_fn(|cx| {
                    assert!(runner.as_mut().poll(cx).is_pending());
                    Poll::Ready(())
                })
                .await;
                drop(runner);
                assert!(weak.upgrade().is_some());
                weak
            })
            .await;
        local.await;
        assert!(weak.upgrade().is_none());
    }
}
