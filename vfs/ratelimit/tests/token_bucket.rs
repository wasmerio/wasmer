use std::sync::{Arc, Mutex};
use std::time::Duration;

use vfs_ratelimit::{
    AcquireError, AcquireOptions, BurstConfig, IoClass, IoCost, Rate, RateLimitConfig, SyncWait,
    TokenBucketLimiter,
};

struct MockTime {
    now: Mutex<std::time::Instant>,
}

impl MockTime {
    fn new() -> Self {
        Self {
            now: Mutex::new(std::time::Instant::now()),
        }
    }

    fn advance(&self, dur: Duration) {
        let mut now = self.now.lock().unwrap();
        *now = *now + dur;
    }
}

impl SyncWait for MockTime {
    fn sleep(&self, dur: Duration) {
        self.advance(dur);
    }

    fn now(&self) -> std::time::Instant {
        *self.now.lock().unwrap()
    }
}

fn cost_meta() -> IoCost {
    IoCost {
        class: IoClass::Meta,
        ops: 1,
        bytes: 0,
    }
}

#[test]
fn token_refill_correctness() {
    let clock = Arc::new(MockTime::new());
    let limiter = TokenBucketLimiter::with_clock(
        RateLimitConfig {
            iops: Some(Rate { per_sec: 10 }),
            burst: BurstConfig {
                ops_burst: 0,
                bytes_burst: 0,
            },
            ..RateLimitConfig::default()
        },
        clock.clone(),
    );

    let opts = AcquireOptions::default();
    for _ in 0..10 {
        limiter
            .try_acquire(cost_meta(), &opts)
            .expect("initial tokens");
    }
    assert_eq!(
        limiter.try_acquire(cost_meta(), &opts),
        Err(AcquireError::WouldBlock)
    );

    clock.advance(Duration::from_millis(100));
    assert!(limiter.try_acquire(cost_meta(), &opts).is_ok());
}

#[test]
fn iops_throttling() {
    let clock = Arc::new(MockTime::new());
    let limiter = TokenBucketLimiter::with_clock(
        RateLimitConfig {
            iops: Some(Rate { per_sec: 10 }),
            burst: BurstConfig {
                ops_burst: 0,
                bytes_burst: 0,
            },
            ..RateLimitConfig::default()
        },
        clock.clone(),
    );

    let opts = AcquireOptions::default();
    let mut ok = 0;
    for _ in 0..20 {
        if limiter.try_acquire(cost_meta(), &opts).is_ok() {
            ok += 1;
        }
    }
    assert_eq!(ok, 10);
}

#[test]
fn bandwidth_throttling() {
    let clock = Arc::new(MockTime::new());
    let limiter = TokenBucketLimiter::with_clock(
        RateLimitConfig {
            read_bps: Some(Rate { per_sec: 1000 }),
            burst: BurstConfig {
                ops_burst: 0,
                bytes_burst: 0,
            },
            ..RateLimitConfig::default()
        },
        clock.clone(),
    );

    let opts = AcquireOptions::default();
    let cost = IoCost {
        class: IoClass::Read,
        ops: 1,
        bytes: 2000,
    };
    assert_eq!(
        limiter.try_acquire(cost, &opts),
        Err(AcquireError::WouldBlock)
    );
}

#[test]
fn nonblocking_behavior() {
    let clock = Arc::new(MockTime::new());
    let limiter = TokenBucketLimiter::with_clock(
        RateLimitConfig {
            iops: Some(Rate { per_sec: 1 }),
            burst: BurstConfig {
                ops_burst: 0,
                bytes_burst: 0,
            },
            ..RateLimitConfig::default()
        },
        clock.clone(),
    );

    let opts = AcquireOptions::default();
    limiter
        .try_acquire(cost_meta(), &opts)
        .expect("consume token");

    let nonblocking = AcquireOptions {
        nonblocking: true,
        ..AcquireOptions::default()
    };
    assert_eq!(
        limiter.try_acquire(cost_meta(), &nonblocking),
        Err(AcquireError::WouldBlock)
    );
}

#[test]
fn timeout_behavior() {
    let clock = Arc::new(MockTime::new());
    let limiter = TokenBucketLimiter::with_clock(
        RateLimitConfig {
            iops: Some(Rate { per_sec: 1 }),
            burst: BurstConfig {
                ops_burst: 0,
                bytes_burst: 0,
            },
            ..RateLimitConfig::default()
        },
        clock.clone(),
    );

    let opts = AcquireOptions::default();
    limiter
        .try_acquire(cost_meta(), &opts)
        .expect("consume token");

    let timed = AcquireOptions {
        nonblocking: false,
        timeout: Some(Duration::from_millis(10)),
        key: None,
    };
    assert_eq!(
        limiter.acquire_blocking(cost_meta(), &timed, clock.as_ref()),
        Err(AcquireError::TimedOut)
    );
}
