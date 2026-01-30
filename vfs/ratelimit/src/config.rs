use crate::{FairLimiter, RateLimiter, TokenBucketLimiter};
use std::sync::Arc;
#[derive(Clone, Debug)]
pub struct RateLimitConfig {
    /// Optional IOPS: operations per second.
    pub iops: Option<Rate>,
    /// Optional read bandwidth bytes/sec.
    pub read_bps: Option<Rate>,
    /// Optional write bandwidth bytes/sec.
    pub write_bps: Option<Rate>,
    /// Optional meta iops (can share iops if omitted).
    pub meta_iops: Option<Rate>,
    /// Burst controls (tokens can accumulate up to burst).
    pub burst: BurstConfig,
    /// Fairness (optional).
    pub fairness: FairnessConfig,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            iops: None,
            read_bps: None,
            write_bps: None,
            meta_iops: None,
            burst: BurstConfig::default(),
            fairness: FairnessConfig::default(),
        }
    }
}

impl RateLimitConfig {
    pub fn build_limiter(self) -> Arc<dyn RateLimiter> {
        let fairness = self.fairness.clone();
        let base: Arc<dyn RateLimiter> = Arc::new(TokenBucketLimiter::new(self));
        match fairness {
            FairnessConfig::None => base,
            FairnessConfig::PerKeyRoundRobin { .. } | FairnessConfig::Weighted { .. } => {
                Arc::new(FairLimiter::new(base, fairness))
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Rate {
    pub per_sec: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct BurstConfig {
    /// Max extra ops that can accumulate beyond steady rate.
    pub ops_burst: u32,
    /// Max extra bytes that can accumulate beyond steady rate.
    pub bytes_burst: u64,
}

impl Default for BurstConfig {
    fn default() -> Self {
        Self {
            ops_burst: 0,
            bytes_burst: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub enum FairnessConfig {
    None,
    /// Round-robin across keys (handle/context/uid).
    PerKeyRoundRobin {
        max_keys: usize,
    },
    /// Weighted fairness; keys have weights provided externally.
    Weighted {
        max_keys: usize,
    },
}

impl Default for FairnessConfig {
    fn default() -> Self {
        FairnessConfig::None
    }
}
