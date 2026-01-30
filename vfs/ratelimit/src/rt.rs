use crate::time::{Instant, TimeSource};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

pub trait SyncWait: Send + Sync {
    fn sleep(&self, dur: Duration);
    fn now(&self) -> Instant;
}

pub trait AsyncWait: Send + Sync {
    fn now(&self) -> Instant;
    fn sleep<'a>(&'a self, dur: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
    fn is_cancelled(&self) -> bool;
}

impl<T> TimeSource for T
where
    T: SyncWait + ?Sized,
{
    fn now(&self) -> Instant {
        SyncWait::now(self)
    }
}

pub struct StdSyncWait;

impl SyncWait for StdSyncWait {
    fn sleep(&self, dur: Duration) {
        std::thread::sleep(dur);
    }

    fn now(&self) -> Instant {
        Instant::now()
    }
}

pub struct NoopAsyncWait;

impl AsyncWait for NoopAsyncWait {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep<'a>(&'a self, dur: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            std::thread::sleep(dur);
        })
    }

    fn is_cancelled(&self) -> bool {
        false
    }
}
