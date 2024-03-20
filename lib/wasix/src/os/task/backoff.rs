use std::{
    collections::HashMap,
    pin::Pin,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

use futures::{Future, FutureExt};
use wasmer_wasix_types::wasi::Snapshot0Clockid;

use crate::{syscalls::platform_clock_time_get, VirtualTaskManager, WasiProcess};

use super::process::LockableWasiProcessInner;

/// Represents the CPU backoff properties for a process
/// which will be used to determine if the CPU should be
/// throttled or not
#[derive(Debug)]
pub struct WasiProcessCpuBackoff {
    /// Referenced list of wakers that will be triggered
    /// when the process goes active again due to a token
    /// being acquired
    cpu_backoff_wakers: HashMap<u64, Waker>,
    /// Seed used to register CPU release wakers
    cpu_backoff_waker_seed: u64,
    /// The amount of CPU backoff time we are currently waiting
    cpu_backoff_time: Duration,
    /// When the backoff is reset the cool-off period will keep
    /// things running for a short period of time extra
    cpu_run_cool_off: u128,
    /// Maximum amount of CPU backoff time before it starts capping
    max_cpu_backoff_time: Duration,
    /// Amount of time the CPU should cool-off after exiting run
    /// before it begins a backoff
    max_cpu_cool_off_time: Duration,
}

impl WasiProcessCpuBackoff {
    pub fn new(max_cpu_backoff_time: Duration, max_cpu_cool_off_time: Duration) -> Self {
        Self {
            cpu_backoff_wakers: Default::default(),
            cpu_backoff_waker_seed: 0,
            cpu_backoff_time: Duration::ZERO,
            cpu_run_cool_off: 0,
            max_cpu_backoff_time,
            max_cpu_cool_off_time,
        }
    }
}

#[derive(Debug)]
pub struct CpuRunToken {
    tokens: Arc<AtomicU32>,
}

impl Drop for CpuRunToken {
    fn drop(&mut self) {
        self.tokens.fetch_sub(1, Ordering::SeqCst);
    }
}

pub struct CpuBackoffToken {
    /// The amount of CPU backoff time we are currently waiting
    cpu_backoff_time: Duration,
    /// How long should the CPU backoff for
    wait: Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>>,
    /// ID used to unregister the wakers
    waker_id: Option<u64>,
    /// The inner protected region of the process with a conditional
    /// variable that is used for coordination such as checksums.
    inner: LockableWasiProcessInner,
}

impl CpuBackoffToken {
    pub fn backoff_time(&self) -> Duration {
        self.cpu_backoff_time
    }
}

impl Future for CpuBackoffToken {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let inner = self.inner.clone();
        let mut inner = inner.0.lock().unwrap();

        // Registering the waker will unregister any previous wakers
        // so that we don't go into an endless memory growth situation
        if let Some(waker_id) = self.waker_id.take() {
            if inner.backoff.cpu_backoff_wakers.remove(&waker_id).is_none() {
                // if we did not remove the waker, then someone else did
                // which means we were woken and should exit the backoff phase
                return Poll::Ready(());
            }
        }

        // Register ourselves as a waker to be woken
        let id = inner.backoff.cpu_backoff_waker_seed + 1;
        inner.backoff.cpu_backoff_waker_seed = id;
        inner
            .backoff
            .cpu_backoff_wakers
            .insert(id, cx.waker().clone());

        // Now poll the waiting period
        let ret = self.wait.poll_unpin(cx);

        // If we have reached the end of the wait period
        // then we need to exponentially grow it any future
        // backoff's so that it gets slower
        if ret.is_ready() && self.cpu_backoff_time == inner.backoff.cpu_backoff_time {
            inner.backoff.cpu_backoff_time *= 2;
            if inner.backoff.cpu_backoff_time > inner.backoff.max_cpu_backoff_time {
                inner.backoff.cpu_backoff_time = inner.backoff.max_cpu_backoff_time;
            }
        }

        ret
    }
}

impl Drop for CpuBackoffToken {
    fn drop(&mut self) {
        if let Some(waker_id) = self.waker_id.take() {
            let mut inner = self.inner.0.lock().unwrap();
            inner.backoff.cpu_backoff_wakers.remove(&waker_id);
        }
    }
}

impl WasiProcess {
    // Releases the CPU backoff (if one is active)
    pub fn acquire_cpu_run_token(&self) -> CpuRunToken {
        self.cpu_run_tokens.fetch_add(1, Ordering::SeqCst);

        let mut inner = self.inner.0.lock().unwrap();
        for (_, waker) in inner.backoff.cpu_backoff_wakers.iter() {
            waker.wake_by_ref();
        }
        inner.backoff.cpu_backoff_wakers.clear();
        inner.backoff.cpu_backoff_time = Duration::ZERO;
        inner.backoff.cpu_run_cool_off = 0;

        CpuRunToken {
            tokens: self.cpu_run_tokens.clone(),
        }
    }

    // Determine if we should do a CPU backoff
    pub fn acquire_cpu_backoff_token(
        &self,
        tasks: &Arc<dyn VirtualTaskManager>,
    ) -> Option<CpuBackoffToken> {
        // If run tokens are held then we should allow executing to
        // continue at its top pace
        if self.cpu_run_tokens.load(Ordering::SeqCst) > 0 {
            return None;
        }

        let cpu_backoff_time = {
            let mut inner = self.inner.0.lock().unwrap();

            // check again as it might have changed (race condition)
            if self.cpu_run_tokens.load(Ordering::SeqCst) > 0 {
                return None;
            }

            // Check if a cool-off-period has passed
            let now =
                platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000).unwrap() as u128;
            if inner.backoff.cpu_run_cool_off == 0 {
                inner.backoff.cpu_run_cool_off =
                    now + (1_000_000 * inner.backoff.max_cpu_cool_off_time.as_millis());
            }
            if now <= inner.backoff.cpu_run_cool_off {
                return None;
            }

            // The amount of time we wait will be increased when a full
            // time slice is executed
            if inner.backoff.cpu_backoff_time == Duration::ZERO {
                inner.backoff.cpu_backoff_time = Duration::from_millis(1);
            }
            inner.backoff.cpu_backoff_time
        };
        let how_long = tasks.sleep_now(cpu_backoff_time);

        Some(CpuBackoffToken {
            cpu_backoff_time,
            wait: how_long,
            waker_id: None,
            inner: self.inner.clone(),
        })
    }
}
