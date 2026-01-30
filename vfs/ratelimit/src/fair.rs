use crate::config::FairnessConfig;
use crate::cost::IoCost;
use crate::limiter::{AcquireError, AcquireOptions, AcquireResult, LimiterKey, RateLimiter};
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

const STALE_AFTER: Duration = Duration::from_secs(5);

pub struct FairLimiter {
    inner: Arc<dyn RateLimiter>,
    config: FairnessConfig,
    state: Mutex<FairState>,
}

struct FairState {
    queue: VecDeque<LimiterKey>,
    last_seen: HashMap<LimiterKey, Instant>,
}

impl FairLimiter {
    pub fn new(inner: Arc<dyn RateLimiter>, config: FairnessConfig) -> Self {
        Self {
            inner,
            config,
            state: Mutex::new(FairState {
                queue: VecDeque::new(),
                last_seen: HashMap::new(),
            }),
        }
    }

    fn max_keys(&self) -> Option<usize> {
        match self.config {
            FairnessConfig::None => None,
            FairnessConfig::PerKeyRoundRobin { max_keys } => Some(max_keys),
            FairnessConfig::Weighted { max_keys } => Some(max_keys),
        }
    }

    fn cleanup_stale(state: &mut FairState) {
        let now = Instant::now();
        while let Some(front) = state.queue.front() {
            let stale = state
                .last_seen
                .get(front)
                .map(|seen| now.duration_since(*seen) > STALE_AFTER)
                .unwrap_or(true);
            if stale {
                let key = state.queue.pop_front().expect("front exists");
                state.last_seen.remove(&key);
            } else {
                break;
            }
        }
    }

    fn enqueue_key(&self, state: &mut FairState, key: LimiterKey) -> AcquireResult {
        let max_keys = self.max_keys().unwrap_or(usize::MAX);
        if !state.last_seen.contains_key(&key) {
            if state.queue.len() >= max_keys {
                return Err(AcquireError::WouldBlock);
            }
            state.queue.push_back(key.clone());
        }
        state.last_seen.insert(key, Instant::now());
        Ok(())
    }
}

impl RateLimiter for FairLimiter {
    fn try_acquire(&self, cost: IoCost, opts: &AcquireOptions) -> AcquireResult {
        if matches!(self.config, FairnessConfig::None) {
            return self.inner.try_acquire(cost, opts);
        }
        let key = match opts.key.clone() {
            Some(key) => key,
            None => return self.inner.try_acquire(cost, opts),
        };

        {
            let mut state = self.state.lock();
            Self::cleanup_stale(&mut state);
            if state.queue.is_empty() {
                drop(state);
                let result = self.inner.try_acquire(cost, opts);
                if matches!(result, Err(AcquireError::WouldBlock)) {
                    let mut state = self.state.lock();
                    Self::cleanup_stale(&mut state);
                    let _ = self.enqueue_key(&mut state, key);
                }
                return result;
            }

            self.enqueue_key(&mut state, key.clone())?;
            if state.queue.front() != Some(&key) {
                return Err(AcquireError::WouldBlock);
            }
        }

        let result = self.inner.try_acquire(cost, opts);
        if result.is_ok() {
            let mut state = self.state.lock();
            if state.queue.front() == Some(&key) {
                state.queue.pop_front();
                state.last_seen.remove(&key);
            }
        }
        result
    }
}
