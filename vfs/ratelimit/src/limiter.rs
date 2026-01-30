use crate::cost::IoCost;
use crate::rt::{AsyncWait, SyncWait};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

pub type AcquireResult = Result<(), AcquireError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquireError {
    WouldBlock,
    TimedOut,
    Cancelled,
    Misconfigured,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LimiterKey {
    Handle(u64),
    Context(u64),
    Uid(u32),
    Mount(u32),
    Named(Arc<str>),
}

#[derive(Clone, Debug)]
pub struct AcquireOptions {
    /// If true: do not wait. Return WouldBlock if insufficient capacity.
    pub nonblocking: bool,
    /// Optional: bounded wait.
    pub timeout: Option<Duration>,
    /// Used for fairness grouping.
    pub key: Option<LimiterKey>,
}

impl Default for AcquireOptions {
    fn default() -> Self {
        Self {
            nonblocking: false,
            timeout: None,
            key: None,
        }
    }
}

pub trait RateLimiter: Send + Sync {
    /// Fast path: try to take capacity immediately.
    fn try_acquire(&self, cost: IoCost, opts: &AcquireOptions) -> AcquireResult;

    /// Slow path: wait until capacity is available or timeout/cancel occurs.
    fn acquire_blocking(
        &self,
        cost: IoCost,
        opts: &AcquireOptions,
        rt: &dyn SyncWait,
    ) -> AcquireResult {
        if opts.nonblocking {
            return self.try_acquire(cost, opts);
        }

        let start = rt.now();
        loop {
            match self.try_acquire(cost, opts) {
                Ok(()) => return Ok(()),
                Err(AcquireError::WouldBlock) => {}
                Err(err) => return Err(err),
            }

            if let Some(timeout) = opts.timeout {
                let elapsed = rt.now().saturating_duration_since(start);
                if elapsed >= timeout {
                    return Err(AcquireError::TimedOut);
                }
            }

            rt.sleep(Duration::from_millis(1));
        }
    }

    /// Async wait variant using runtime hooks.
    fn acquire_async<'a>(
        &'a self,
        cost: IoCost,
        opts: AcquireOptions,
        rt: &'a dyn AsyncWait,
    ) -> Pin<Box<dyn Future<Output = AcquireResult> + Send + 'a>> {
        Box::pin(async move {
            if opts.nonblocking {
                return self.try_acquire(cost, &opts);
            }

            let start = rt.now();
            loop {
                match self.try_acquire(cost, &opts) {
                    Ok(()) => return Ok(()),
                    Err(AcquireError::WouldBlock) => {}
                    Err(err) => return Err(err),
                }

                if rt.is_cancelled() {
                    return Err(AcquireError::Cancelled);
                }

                if let Some(timeout) = opts.timeout {
                    let elapsed = rt.now().saturating_duration_since(start);
                    if elapsed >= timeout {
                        return Err(AcquireError::TimedOut);
                    }
                }

                rt.sleep(Duration::from_millis(1)).await;
            }
        })
    }
}

#[derive(Clone, Default)]
pub struct LimiterChain {
    pub global: Option<Arc<dyn RateLimiter>>,
    pub mount: Option<Arc<dyn RateLimiter>>,
    pub fs: Option<Arc<dyn RateLimiter>>,
}

impl LimiterChain {
    pub fn is_empty(&self) -> bool {
        self.global.is_none() && self.mount.is_none() && self.fs.is_none()
    }

    pub fn try_acquire(&self, cost: IoCost, opts: &AcquireOptions) -> AcquireResult {
        for limiter in [&self.global, &self.mount, &self.fs] {
            if let Some(limiter) = limiter.as_deref() {
                limiter.try_acquire(cost, opts)?;
            }
        }
        Ok(())
    }

    pub fn acquire_blocking(
        &self,
        cost: IoCost,
        opts: &AcquireOptions,
        rt: &dyn SyncWait,
    ) -> AcquireResult {
        for limiter in [&self.global, &self.mount, &self.fs] {
            if let Some(limiter) = limiter.as_deref() {
                limiter.acquire_blocking(cost, opts, rt)?;
            }
        }
        Ok(())
    }

    pub fn acquire_async<'a>(
        &'a self,
        cost: IoCost,
        opts: AcquireOptions,
        rt: &'a dyn AsyncWait,
    ) -> Pin<Box<dyn Future<Output = AcquireResult> + Send + 'a>> {
        Box::pin(async move {
            for limiter in [&self.global, &self.mount, &self.fs] {
                if let Some(limiter) = limiter.as_deref() {
                    limiter.acquire_async(cost, opts.clone(), rt).await?;
                }
            }
            Ok(())
        })
    }
}
