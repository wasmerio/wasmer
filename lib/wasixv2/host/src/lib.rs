//! What the kernel needs from the host: an executor, clocks, timers and entropy.

#[cfg(feature = "tokio")]
mod tokio_host;

use std::{future::Future, io, pin::Pin, sync::Arc, time::Duration};

#[cfg(feature = "tokio")]
pub use tokio_host::TokioHost;

/// Boxed future without a `Send` bound, matching wasmer's async host calls.
pub type LocalFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// A raw byte stream from the host, such as the embedder's stdin.
pub trait ByteSource: Send + Sync {
    /// Reads into `buf`; `Ok(0)` is end of stream.
    fn read<'a>(&'a self, buf: &'a mut [u8]) -> LocalFuture<'a, io::Result<usize>>;
}

/// A raw byte stream to the host, such as the embedder's stdout.
pub trait ByteSink: Send + Sync {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>>;
}

/// The host streams a new process gets as descriptors 0, 1 and 2.
pub struct Stdio {
    pub stdin: Arc<dyn ByteSource>,
    pub stdout: Arc<dyn ByteSink>,
    pub stderr: Arc<dyn ByteSink>,
}

/// Clocks, timers and entropy.
pub trait Host: Send + Sync {
    /// Time since the Unix epoch, for `CLOCK_REALTIME`.
    fn realtime(&self) -> Duration;
    /// Time since a fixed point in the past, for `CLOCK_MONOTONIC`. Never goes backwards.
    fn monotonic(&self) -> Duration;
    /// Completes once `duration` has passed, without blocking the calling thread.
    fn sleep(&self, duration: Duration) -> LocalFuture<'_, ()>;
    /// Fills `buf` with cryptographically secure random bytes.
    fn random(&self, buf: &mut [u8]);
}
