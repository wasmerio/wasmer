use crate::config::{Rate, RateLimitConfig};
use crate::cost::{IoClass, IoCost};
use crate::limiter::{AcquireError, AcquireOptions, AcquireResult, RateLimiter};
use crate::rt::{AsyncWait, SyncWait};
use crate::time::{Instant, StdTime, TimeSource};
use parking_lot::Mutex;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

const MIN_SLEEP: Duration = Duration::from_micros(250);
const MAX_SLEEP: Duration = Duration::from_millis(50);

#[derive(Clone)]
pub struct TokenBucketLimiter {
    config: RateLimitConfig,
    clock: Arc<dyn TimeSource>,
    state: Arc<Mutex<BucketState>>,
}

#[derive(Clone, Copy)]
struct BucketConfig {
    rate_per_sec: f64,
    max_tokens: f64,
}

struct BucketState {
    last: Instant,
    ops_tokens: f64,
    meta_ops_tokens: f64,
    read_tokens: f64,
    write_tokens: f64,
}

#[derive(Default)]
struct RequiredTokens {
    ops: Option<f64>,
    meta_ops: Option<f64>,
    read: Option<f64>,
    write: Option<f64>,
}

impl TokenBucketLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self::with_clock(config, Arc::new(StdTime))
    }

    pub fn with_clock(config: RateLimitConfig, clock: Arc<dyn TimeSource>) -> Self {
        let now = clock.now();
        let state = BucketState {
            last: now,
            ops_tokens: Self::initial_tokens(config.iops, config.burst.ops_burst as f64),
            meta_ops_tokens: Self::initial_tokens(config.meta_iops, config.burst.ops_burst as f64),
            read_tokens: Self::initial_tokens(config.read_bps, config.burst.bytes_burst as f64),
            write_tokens: Self::initial_tokens(config.write_bps, config.burst.bytes_burst as f64),
        };
        Self {
            config,
            clock,
            state: Arc::new(Mutex::new(state)),
        }
    }

    fn initial_tokens(rate: Option<Rate>, burst: f64) -> f64 {
        let Some(rate) = rate else {
            return 0.0;
        };
        let base = rate.per_sec as f64;
        if rate.per_sec == 0 {
            return 0.0;
        }
        base + burst
    }

    fn bucket_config(rate: Option<Rate>, burst: f64) -> Option<BucketConfig> {
        let rate = rate?;
        if rate.per_sec == 0 {
            return Some(BucketConfig {
                rate_per_sec: 0.0,
                max_tokens: 0.0,
            });
        }
        Some(BucketConfig {
            rate_per_sec: rate.per_sec as f64,
            max_tokens: (rate.per_sec as f64) + burst,
        })
    }

    fn refill(state: &mut BucketState, now: Instant, config: &RateLimitConfig) {
        let elapsed = now.saturating_duration_since(state.last);
        if elapsed.is_zero() {
            return;
        }
        let elapsed_secs = elapsed.as_secs_f64();
        let burst_ops = config.burst.ops_burst as f64;
        let burst_bytes = config.burst.bytes_burst as f64;

        if let Some(cfg) = Self::bucket_config(config.iops, burst_ops) {
            state.ops_tokens =
                (state.ops_tokens + elapsed_secs * cfg.rate_per_sec).min(cfg.max_tokens);
        }
        if let Some(cfg) = Self::bucket_config(config.meta_iops, burst_ops) {
            state.meta_ops_tokens =
                (state.meta_ops_tokens + elapsed_secs * cfg.rate_per_sec).min(cfg.max_tokens);
        }
        if let Some(cfg) = Self::bucket_config(config.read_bps, burst_bytes) {
            state.read_tokens =
                (state.read_tokens + elapsed_secs * cfg.rate_per_sec).min(cfg.max_tokens);
        }
        if let Some(cfg) = Self::bucket_config(config.write_bps, burst_bytes) {
            state.write_tokens =
                (state.write_tokens + elapsed_secs * cfg.rate_per_sec).min(cfg.max_tokens);
        }

        state.last = now;
    }

    fn required_tokens(&self, cost: IoCost) -> RequiredTokens {
        let mut required = RequiredTokens::default();
        if cost.ops > 0 {
            let ops = cost.ops as f64;
            match cost.class {
                IoClass::Meta | IoClass::OpenClose => {
                    if self.config.meta_iops.is_some() {
                        required.meta_ops = Some(ops);
                    } else if self.config.iops.is_some() {
                        required.ops = Some(ops);
                    }
                }
                _ => {
                    if self.config.iops.is_some() {
                        required.ops = Some(ops);
                    }
                }
            }
        }

        if cost.bytes > 0 {
            let bytes = cost.bytes as f64;
            match cost.class {
                IoClass::Read | IoClass::ReadDir => {
                    if self.config.read_bps.is_some() {
                        required.read = Some(bytes);
                    }
                }
                IoClass::Write => {
                    if self.config.write_bps.is_some() {
                        required.write = Some(bytes);
                    }
                }
                _ => {}
            }
        }

        required
    }

    fn validate_config(required: &RequiredTokens, config: &RateLimitConfig) -> AcquireResult {
        let zero_rate = |rate: Option<Rate>| rate.map(|r| r.per_sec == 0).unwrap_or(false);
        if required.ops.is_some() && zero_rate(config.iops) {
            return Err(AcquireError::Misconfigured);
        }
        if required.meta_ops.is_some() && zero_rate(config.meta_iops) {
            return Err(AcquireError::Misconfigured);
        }
        if required.read.is_some() && zero_rate(config.read_bps) {
            return Err(AcquireError::Misconfigured);
        }
        if required.write.is_some() && zero_rate(config.write_bps) {
            return Err(AcquireError::Misconfigured);
        }
        Ok(())
    }

    fn can_consume(state: &BucketState, required: &RequiredTokens) -> bool {
        if let Some(ops) = required.ops {
            if state.ops_tokens < ops {
                return false;
            }
        }
        if let Some(ops) = required.meta_ops {
            if state.meta_ops_tokens < ops {
                return false;
            }
        }
        if let Some(bytes) = required.read {
            if state.read_tokens < bytes {
                return false;
            }
        }
        if let Some(bytes) = required.write {
            if state.write_tokens < bytes {
                return false;
            }
        }
        true
    }

    fn apply_cost(state: &mut BucketState, required: &RequiredTokens) {
        if let Some(ops) = required.ops {
            state.ops_tokens -= ops;
        }
        if let Some(ops) = required.meta_ops {
            state.meta_ops_tokens -= ops;
        }
        if let Some(bytes) = required.read {
            state.read_tokens -= bytes;
        }
        if let Some(bytes) = required.write {
            state.write_tokens -= bytes;
        }
    }

    fn compute_wait(
        required: &RequiredTokens,
        state: &BucketState,
        config: &RateLimitConfig,
    ) -> Duration {
        let mut wait = 0.0f64;
        let burst_ops = config.burst.ops_burst as f64;
        let burst_bytes = config.burst.bytes_burst as f64;

        let mut update_wait = |missing: f64, rate: Option<Rate>, burst: f64| {
            if missing <= 0.0 {
                return;
            }
            if let Some(cfg) = Self::bucket_config(rate, burst) {
                if cfg.rate_per_sec > 0.0 {
                    wait = wait.max(missing / cfg.rate_per_sec);
                }
            }
        };

        if let Some(ops) = required.ops {
            update_wait(ops - state.ops_tokens, config.iops, burst_ops);
        }
        if let Some(ops) = required.meta_ops {
            update_wait(ops - state.meta_ops_tokens, config.meta_iops, burst_ops);
        }
        if let Some(bytes) = required.read {
            update_wait(bytes - state.read_tokens, config.read_bps, burst_bytes);
        }
        if let Some(bytes) = required.write {
            update_wait(bytes - state.write_tokens, config.write_bps, burst_bytes);
        }

        if wait <= 0.0 {
            MIN_SLEEP
        } else {
            Duration::from_secs_f64(wait)
        }
    }
}

impl RateLimiter for TokenBucketLimiter {
    fn try_acquire(&self, cost: IoCost, _opts: &AcquireOptions) -> AcquireResult {
        let required = self.required_tokens(cost);
        Self::validate_config(&required, &self.config)?;

        let now = self.clock.now();
        let mut state = self.state.lock();
        Self::refill(&mut state, now, &self.config);

        if !Self::can_consume(&state, &required) {
            return Err(AcquireError::WouldBlock);
        }

        Self::apply_cost(&mut state, &required);
        Ok(())
    }

    fn acquire_blocking(
        &self,
        cost: IoCost,
        opts: &AcquireOptions,
        rt: &dyn SyncWait,
    ) -> AcquireResult {
        if opts.nonblocking {
            return self.try_acquire(cost, opts);
        }

        let required = self.required_tokens(cost);
        Self::validate_config(&required, &self.config)?;
        let start = rt.now();

        loop {
            let now = rt.now();
            let mut state = self.state.lock();
            Self::refill(&mut state, now, &self.config);

            if Self::can_consume(&state, &required) {
                Self::apply_cost(&mut state, &required);
                return Ok(());
            }
            drop(state);

            if let Some(timeout) = opts.timeout {
                let elapsed = now.saturating_duration_since(start);
                if elapsed >= timeout {
                    return Err(AcquireError::TimedOut);
                }
            }

            let mut wait = {
                let state = self.state.lock();
                Self::compute_wait(&required, &state, &self.config)
            };
            if wait < MIN_SLEEP {
                wait = MIN_SLEEP;
            }
            if wait > MAX_SLEEP {
                wait = MAX_SLEEP;
            }
            if let Some(timeout) = opts.timeout {
                let elapsed = rt.now().saturating_duration_since(start);
                let remaining = timeout.saturating_sub(elapsed);
                if remaining.is_zero() {
                    return Err(AcquireError::TimedOut);
                }
                if wait > remaining {
                    wait = remaining;
                }
            }
            rt.sleep(wait);
        }
    }

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

            let required = self.required_tokens(cost);
            Self::validate_config(&required, &self.config)?;
            let start = rt.now();

            loop {
                let now = rt.now();
                {
                    let mut state = self.state.lock();
                    Self::refill(&mut state, now, &self.config);
                    if Self::can_consume(&state, &required) {
                        Self::apply_cost(&mut state, &required);
                        return Ok(());
                    }
                }

                if rt.is_cancelled() {
                    return Err(AcquireError::Cancelled);
                }

                if let Some(timeout) = opts.timeout {
                    let elapsed = now.saturating_duration_since(start);
                    if elapsed >= timeout {
                        return Err(AcquireError::TimedOut);
                    }
                }

                let mut wait = {
                    let state = self.state.lock();
                    Self::compute_wait(&required, &state, &self.config)
                };
                if wait < MIN_SLEEP {
                    wait = MIN_SLEEP;
                }
                if wait > MAX_SLEEP {
                    wait = MAX_SLEEP;
                }
                if let Some(timeout) = opts.timeout {
                    let elapsed = rt.now().saturating_duration_since(start);
                    let remaining = timeout.saturating_sub(elapsed);
                    if remaining.is_zero() {
                        return Err(AcquireError::TimedOut);
                    }
                    if wait > remaining {
                        wait = remaining;
                    }
                }

                rt.sleep(wait).await;
            }
        })
    }
}
