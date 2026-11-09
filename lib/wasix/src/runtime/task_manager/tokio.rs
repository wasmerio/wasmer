use std::sync::{Mutex, PoisonError};
use std::task::{Context, Poll};
use std::{num::NonZeroUsize, pin::Pin, sync::Arc, time::Duration};

use futures::{Future, future::BoxFuture};
use tokio::runtime::{Handle, Runtime};
use virtual_mio::block_on;

use crate::runtime::{SpawnType, task_manager::TaskWasmCallbacks};
use crate::{WasiFunctionEnv, os::task::thread::WasiThreadError};

use super::{SpawnMemoryTypeOrStore, TaskWasm, TaskWasmRunProperties, VirtualTaskManager};

#[derive(Debug, Clone)]
pub enum RuntimeOrHandle {
    Handle(Handle),
    Runtime(Handle, Arc<Mutex<Option<Runtime>>>),
}
impl From<Handle> for RuntimeOrHandle {
    fn from(value: Handle) -> Self {
        Self::Handle(value)
    }
}
impl From<Runtime> for RuntimeOrHandle {
    fn from(value: Runtime) -> Self {
        Self::Runtime(value.handle().clone(), Arc::new(Mutex::new(Some(value))))
    }
}

impl Drop for RuntimeOrHandle {
    fn drop(&mut self) {
        if let Self::Runtime(_, runtime) = self
            && let Some(h) = runtime.lock().unwrap().take()
        {
            h.shutdown_timeout(Duration::from_secs(0))
        }
    }
}

impl RuntimeOrHandle {
    pub fn handle(&self) -> &Handle {
        match self {
            Self::Handle(h) => h,
            Self::Runtime(h, _) => h,
        }
    }
}

#[derive(Clone)]
pub struct ThreadPool {
    inner: rusty_pool::ThreadPool,
}

impl std::ops::Deref for ThreadPool {
    type Target = rusty_pool::ThreadPool;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::fmt::Debug for ThreadPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadPool")
            .field("name", &self.get_name())
            .field("current_worker_count", &self.get_current_worker_count())
            .field("idle_worker_count", &self.get_idle_worker_count())
            .finish()
    }
}

/// A task manager that uses tokio to spawn tasks.
#[derive(Clone, Debug)]
pub struct TokioTaskManager {
    rt: RuntimeOrHandle,
    pool: Arc<ThreadPool>,
}

impl TokioTaskManager {
    pub fn new<I>(rt: I) -> Self
    where
        I: Into<RuntimeOrHandle>,
    {
        let concurrency = std::thread::available_parallelism()
            .unwrap_or(NonZeroUsize::new(1).unwrap())
            .get();
        let max_threads = 200usize.max(concurrency * 100);

        Self {
            rt: rt.into(),
            pool: Arc::new(ThreadPool {
                inner: rusty_pool::Builder::new()
                    .name("TokioTaskManager Thread Pool".to_string())
                    .core_size(max_threads)
                    .max_size(max_threads)
                    .build(),
            }),
        }
    }

    pub fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.rt.handle().clone()
    }

    pub fn pool_handle(&self) -> Arc<ThreadPool> {
        self.pool.clone()
    }
}

impl Default for TokioTaskManager {
    fn default() -> Self {
        Self::new(Handle::current())
    }
}

impl VirtualTaskManager for TokioTaskManager {
    /// See [`VirtualTaskManager::sleep_now`].
    fn sleep_now(&self, time: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + Sync>> {
        let owned_runtime = match &self.rt {
            RuntimeOrHandle::Handle(_) => None,
            RuntimeOrHandle::Runtime(_, runtime) => Some(runtime.clone()),
        };
        Box::pin(RuntimeSleep::new(
            self.runtime_handle(),
            time,
            owned_runtime,
        ))
    }

    /// See [`VirtualTaskManager::task_shared`].
    fn task_shared(
        &self,
        task: Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        self.rt.handle().spawn(async move {
            let fut = task();
            fut.await
        });
        Ok(())
    }

    /// See [`VirtualTaskManager::task_wasm`].
    fn task_wasm(&self, task: TaskWasm) -> Result<(), WasiThreadError> {
        fn env_and_store(
            task: TaskWasm,
        ) -> Result<(WasiFunctionEnv, wasmer::Store, TaskWasmCallbacks), WasiThreadError> {
            let (make_memory, instance_group_data) = match task.spawn_type {
                SpawnType::CreateMemory => (SpawnMemoryTypeOrStore::New, None),
                SpawnType::NewLinkerInstanceGroup(instance_group_data) => {
                    (SpawnMemoryTypeOrStore::New, Some(instance_group_data))
                }
                SpawnType::CreateMemoryOfType(t) => (SpawnMemoryTypeOrStore::Type(t), None),
                SpawnType::AttachMemory(mem) => {
                    let mut store = task.env.runtime().new_store();
                    let memory = mem
                        .try_attach(&mut store)
                        .map_err(WasiThreadError::MemoryCreateFailed)?;
                    (SpawnMemoryTypeOrStore::StoreAndMemory(store, memory), None)
                }
            };

            let (env, store) = WasiFunctionEnv::new_with_store(
                task.module,
                task.env,
                task.globals,
                make_memory,
                task.update_layout,
                task.call_initialize,
                instance_group_data,
            )?;
            Ok((env, store, task.callbacks))
        }

        let (sx, rx) = std::sync::mpsc::channel();

        if task.callbacks.trigger.is_some() {
            tracing::trace!("spawning task_wasm trigger in async pool");
            self.pool.execute(move || {
                let (mut ctx, mut store, callbacks) = match env_and_store(task) {
                    Ok(x) => {
                        sx.send(Ok(())).unwrap();
                        x
                    }
                    Err(c) => {
                        tracing::error!("failed to prepare environment for task_wasm trigger: {c}");
                        sx.send(Err(c)).unwrap();
                        return;
                    }
                };

                let result = {
                    let mut trigger = (callbacks.trigger.unwrap())();
                    let pre_run = callbacks.pre_run;
                    let ctx = &mut ctx;
                    let store = &mut store;
                    block_on(async move {
                        // We wait for either the trigger or for a snapshot to take place
                        let result = loop {
                            let env = ctx.data(store);
                            break tokio::select! {
                                r = &mut trigger => r,
                                _ = env.thread.wait_for_signal() => {
                                    tracing::debug!("wait-for-signal(triggered)");
                                    let mut ctx = ctx.env.clone().into_mut(store);
                                    if let Err(err) =
                                        crate::WasiEnv::do_pending_link_operations(
                                            &mut ctx,
                                            false
                                        ).and_then(|()|
                                            crate::WasiEnv::process_signals_and_exit(&mut ctx)
                                        )
                                    {
                                        match err {
                                            crate::WasiError::Exit(code) => Err(code),
                                            err => {
                                                tracing::error!("failed to process signals - {}", err);
                                                continue;
                                            }
                                        }
                                    } else {
                                        continue;
                                    }
                                }
                                _ = crate::wait_for_snapshot(env) => {
                                    tracing::debug!("wait-for-snapshot(triggered)");
                                    let mut ctx = ctx.env.clone().into_mut(store);
                                    crate::os::task::WasiProcessInner::do_checkpoints_from_outside(&mut ctx);
                                    continue;
                                }
                            };
                        };

                        if let Some(pre_run) = pre_run {
                            pre_run(ctx, store).await;
                        }

                        result
                    })
                };

                // Invoke the callback
                (callbacks.run)(TaskWasmRunProperties {
                    ctx,
                    store,
                    trigger_result: Some(result),
                    recycle: callbacks.recycle,
                });
            });
        } else {
            tracing::trace!("spawning task_wasm in blocking thread");

            // Run the callback on a dedicated thread
            self.pool.execute(move || {
                tracing::trace!("task_wasm started in blocking thread");
                let (mut ctx, mut store, callbacks) = match env_and_store(task) {
                    Ok(x) => {
                        sx.send(Ok(())).unwrap();
                        x
                    }
                    Err(c) => {
                        sx.send(Err(c)).unwrap();
                        return;
                    }
                };

                if let Some(pre_run) = callbacks.pre_run {
                    block_on(pre_run(&mut ctx, &mut store));
                }

                // Invoke the callback
                (callbacks.run)(TaskWasmRunProperties {
                    ctx,
                    store,
                    trigger_result: None,
                    recycle: callbacks.recycle,
                });
            });
        }

        rx.recv()
            .map_err(|_| WasiThreadError::InvalidWasmContext)??;

        Ok(())
    }

    /// See [`VirtualTaskManager::task_dedicated`].
    fn task_dedicated(
        &self,
        task: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        self.pool.execute(move || {
            task();
        });
        Ok(())
    }

    /// See [`VirtualTaskManager::thread_parallelism`].
    fn thread_parallelism(&self) -> Result<usize, WasiThreadError> {
        Ok(std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(8))
    }
}

/// Sleeps on a Tokio runtime without spawning a task.
///
/// Kept for API compatibility: [`SleepNow::enter`] used to spawn a timer task
/// and abort it when dropped. It now polls the timer inline, like
/// [`TokioTaskManager::sleep_now`], and always returns `Ok(())`.
#[derive(Default)]
pub struct SleepNow {
    _private: (),
}

impl SleepNow {
    /// Sleeps for `time` using the timer of the runtime behind `handle`.
    ///
    /// A zero duration yields exactly once. The runtime must have timers
    /// enabled and must not be shut down while the returned future is
    /// pending: polling it after the runtime was dropped panics with "A Tokio
    /// 1.x context was found, but it is being shutdown". Embedders that pass
    /// a handle must keep their runtime alive until all guest threads that
    /// may sleep have joined.
    pub async fn enter(
        &mut self,
        handle: tokio::runtime::Handle,
        time: Duration,
    ) -> Result<(), tokio::task::JoinError> {
        RuntimeSleep::new(handle, time, None).await;
        Ok(())
    }
}

/// Future returned by [`TokioTaskManager::sleep_now`].
///
/// Nearly every blocking syscall races its work against a `sleep_now` future
/// (the deep-sleep timer, `epoll_wait`/`futex_wait`/`poll_oneoff` timeouts,
/// socket timeouts) and drops it as soon as the syscall completes, usually
/// long before the timer fires. The timer is therefore polled inline by
/// whoever awaits the future (typically a WASM thread blocked in
/// [`block_on`]) instead of in a spawned task: spawning and aborting a task
/// per call costs a task allocation plus several cross-thread wake-ups of the
/// runtime's worker threads.
///
/// * The timer is created on the first poll (so it starts then) and is bound
///   to the given runtime, whatever Tokio context the polling thread has.
/// * A zero duration yields exactly once instead of arming a timer: the first
///   poll returns `Pending` with a wake-up already scheduled, the second
///   completes. A zero-length `tokio::time::Sleep` would wait for the next
///   millisecond tick of the timer driver.
/// * Dropping the future deregisters the timer synchronously.
/// * Polling a Tokio timer after its runtime was shut down panics. For a
///   runtime owned by the task manager (`RuntimeOrHandle::Runtime`) the
///   future therefore checks, under the lock that guards the runtime's
///   shutdown, whether it is still running, and completes immediately if not
///   (as the spawned timer task did). A runtime supplied as a
///   [`Handle`] must stay running while sleeps are pending: polling after it
///   was dropped panics with "A Tokio 1.x context was found, but it is being
///   shutdown", so embedders must keep it alive until all guest threads that
///   may sleep have joined.
/// * The runtime must have timers enabled (`enable_time`/`enable_all`);
///   otherwise creating the timer panics.
#[pin_project::pin_project(project = RuntimeSleepProj)]
enum RuntimeSleep {
    Yield {
        yielded: bool,
    },
    Timer {
        // Declared first so the timer is deregistered before anything else
        // is dropped.
        #[pin]
        sleep: Option<tokio::time::Sleep>,
        duration: Duration,
        handle: Handle,
        owned_runtime: Option<Arc<Mutex<Option<Runtime>>>>,
    },
}

impl RuntimeSleep {
    fn new(
        handle: Handle,
        duration: Duration,
        owned_runtime: Option<Arc<Mutex<Option<Runtime>>>>,
    ) -> Self {
        if duration.is_zero() {
            Self::Yield { yielded: false }
        } else {
            Self::Timer {
                sleep: None,
                duration,
                handle,
                owned_runtime,
            }
        }
    }
}

impl Future for RuntimeSleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        match self.project() {
            RuntimeSleepProj::Yield { yielded } => {
                if *yielded {
                    return Poll::Ready(());
                }
                *yielded = true;
                // Tokio's yield defers the wake-up to the end of the current
                // scheduler tick when polled inside a runtime task (so the
                // task cannot starve its siblings) and wakes immediately
                // everywhere else. Its first poll is always `Pending`; the
                // scheduled wake-up survives dropping it.
                let yield_now = std::pin::pin!(tokio::task::yield_now());
                let _ = yield_now.poll(cx);
                Poll::Pending
            }
            RuntimeSleepProj::Timer {
                mut sleep,
                duration,
                handle,
                owned_runtime,
            } => {
                if sleep.is_none() {
                    // `tokio::time::sleep` binds to the runtime of the current
                    // context, which the polling thread may not have.
                    let _guard = handle.enter();
                    sleep.set(Some(tokio::time::sleep(*duration)));
                }
                let sleep = sleep.as_pin_mut().expect("timer was just created");
                match owned_runtime {
                    // The runtime is shut down while this lock is held (see
                    // `RuntimeOrHandle::drop`), so it cannot go away while the
                    // timer is polled.
                    Some(runtime) => {
                        let runtime = runtime.lock().unwrap_or_else(PoisonError::into_inner);
                        if runtime.is_none() {
                            return Poll::Ready(());
                        }
                        sleep.poll(cx)
                    }
                    None => sleep.poll(cx),
                }
            }
        }
    }
}

#[cfg(test)]
mod sleep_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Wake, Waker};
    use std::time::Instant;

    use super::*;

    /// Generous upper bound for timer lateness on a loaded machine.
    const SLACK: Duration = Duration::from_secs(2);

    fn owned_runtime() -> Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
    }

    #[derive(Default)]
    struct CountingWaker {
        wakes: AtomicUsize,
    }

    impl Wake for CountingWaker {
        fn wake(self: Arc<Self>) {
            self.wakes.fetch_add(1, Ordering::SeqCst);
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.wakes.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn poll_once<F: Future + ?Sized>(future: Pin<&mut F>, waker: &Waker) -> Poll<F::Output> {
        future.poll(&mut Context::from_waker(waker))
    }

    fn assert_slept(elapsed: Duration, duration: Duration) {
        assert!(elapsed >= duration, "woke early after {elapsed:?}");
        assert!(elapsed < duration + SLACK, "woke late after {elapsed:?}");
    }

    /// WASM threads poll `sleep_now` from `block_on` on plain OS threads; the
    /// timer must be driven by the task manager's runtime.
    #[test]
    fn fires_on_a_thread_without_tokio_context() {
        let rt = owned_runtime();
        for manager in [
            TokioTaskManager::new(rt.handle().clone()),
            TokioTaskManager::new(owned_runtime()),
        ] {
            let duration = Duration::from_millis(30);
            let elapsed = std::thread::spawn(move || {
                assert!(Handle::try_current().is_err());
                let start = Instant::now();
                block_on(manager.sleep_now(duration));
                start.elapsed()
            })
            .join()
            .unwrap();
            assert_slept(elapsed, duration);
        }
    }

    /// The timer is bound to the task manager's runtime even when polled
    /// inside another runtime that has no timer driver.
    #[test]
    fn fires_when_polled_inside_a_runtime_without_timers() {
        let manager = TokioTaskManager::new(owned_runtime());
        let foreign = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let duration = Duration::from_millis(20);
        let start = Instant::now();
        foreign.block_on(manager.sleep_now(duration));
        assert_slept(start.elapsed(), duration);
    }

    /// Blocking syscalls create, poll and drop a `sleep_now` future each;
    /// none of that may spawn a runtime task.
    #[test]
    fn does_not_spawn_a_task_per_call() {
        let rt = owned_runtime();
        let manager = TokioTaskManager::new(rt.handle().clone());
        let waker = Waker::from(Arc::new(CountingWaker::default()));

        let mut sleeps: Vec<_> = (0..256)
            .map(|_| manager.sleep_now(Duration::from_secs(60)))
            .collect();
        for sleep in &mut sleeps {
            assert!(poll_once(sleep.as_mut(), &waker).is_pending());
        }
        let mut enter = SleepNow::default();
        let mut sleep_now = Box::pin(enter.enter(rt.handle().clone(), Duration::from_secs(60)));
        assert!(poll_once(sleep_now.as_mut(), &waker).is_pending());
        assert_eq!(rt.metrics().num_alive_tasks(), 0);

        drop(sleep_now);
        drop(sleeps);
        assert_eq!(rt.metrics().num_alive_tasks(), 0);
    }

    /// Dropping a pending sleep cancels its timer: the waker is released
    /// right away and never woken.
    #[test]
    fn drop_cancels_the_timer_and_releases_the_waker() {
        let manager = TokioTaskManager::new(owned_runtime());
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));

        let mut sleep = manager.sleep_now(Duration::from_millis(10));
        assert!(poll_once(sleep.as_mut(), &waker).is_pending());
        assert!(Arc::strong_count(&counter) > 2, "timer holds no waker");
        drop(sleep);
        drop(waker);
        assert_eq!(Arc::strong_count(&counter), 1, "waker leaked");

        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(counter.wakes.load(Ordering::SeqCst), 0);
    }

    /// The timer starts on the first poll, not when the future is created.
    #[test]
    fn timer_starts_on_first_poll() {
        let manager = TokioTaskManager::new(owned_runtime());
        let duration = Duration::from_millis(40);
        let sleep = manager.sleep_now(duration);
        std::thread::sleep(duration * 2);
        let start = Instant::now();
        block_on(sleep);
        assert_slept(start.elapsed(), duration);
    }

    /// A zero duration yields once: `Pending` with a wake-up already
    /// scheduled, then `Ready`, without waiting for a timer tick.
    #[test]
    fn zero_duration_yields_exactly_once() {
        let manager = TokioTaskManager::new(owned_runtime());
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));

        let mut sleep = manager.sleep_now(Duration::ZERO);
        assert!(poll_once(sleep.as_mut(), &waker).is_pending());
        assert_eq!(counter.wakes.load(Ordering::SeqCst), 1);
        assert!(poll_once(sleep.as_mut(), &waker).is_ready());
    }

    /// Inside a runtime task a zero sleep yields to the scheduler and the
    /// task is resumed.
    #[test]
    fn zero_duration_yield_resumes_runtime_tasks() {
        let rt = owned_runtime();
        let manager = TokioTaskManager::new(rt.handle().clone());
        let task = rt.spawn(async move {
            for _ in 0..100 {
                manager.sleep_now(Duration::ZERO).await;
            }
        });
        // Fail instead of hanging if a yield's wake-up is lost.
        rt.block_on(async { tokio::time::timeout(SLACK, task).await })
            .expect("zero-duration sleeps did not resume the task")
            .unwrap();
    }

    /// Many threads sleeping concurrently all wake up, and none early.
    #[test]
    fn many_concurrent_sleeps_all_fire() {
        let rt = owned_runtime();
        let manager = Arc::new(TokioTaskManager::new(rt.handle().clone()));
        let threads: Vec<_> = (0..16u64)
            .map(|thread| {
                let manager = manager.clone();
                std::thread::spawn(move || {
                    for i in 0..50u64 {
                        let duration = Duration::from_micros(500 + (thread * 50 + i) * 20);
                        let start = Instant::now();
                        block_on(manager.sleep_now(duration));
                        assert!(start.elapsed() >= duration);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }

        let sleeps: Vec<_> = (1..=500u64)
            .map(|i| manager.sleep_now(Duration::from_micros(i * 40)))
            .collect();
        let start = Instant::now();
        block_on(futures::future::join_all(sleeps));
        assert!(start.elapsed() >= Duration::from_millis(20));
        assert_eq!(rt.metrics().num_alive_tasks(), 0);
    }

    /// When a task manager that owns its runtime is dropped, the runtime is
    /// shut down; pending and new sleeps then complete instead of panicking.
    #[test]
    fn completes_after_owned_runtime_shutdown() {
        let manager = TokioTaskManager::new(owned_runtime());
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));

        let mut pending = manager.sleep_now(Duration::from_secs(60));
        assert!(poll_once(pending.as_mut(), &waker).is_pending());
        let never_polled = manager.sleep_now(Duration::from_secs(60));
        drop(manager);

        assert!(poll_once(pending.as_mut(), &waker).is_ready());
        let start = Instant::now();
        block_on(never_polled);
        assert!(start.elapsed() < SLACK);
    }
}
