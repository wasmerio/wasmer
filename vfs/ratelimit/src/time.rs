pub use std::time::{Duration, Instant};

pub trait TimeSource: Send + Sync {
    fn now(&self) -> Instant;
}

pub struct StdTime;

impl TimeSource for StdTime {
    fn now(&self) -> Instant {
        Instant::now()
    }
}
