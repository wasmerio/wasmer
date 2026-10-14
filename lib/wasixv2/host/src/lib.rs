//! What the kernel needs from the host: an executor, clocks, timers and entropy.

use std::{future::Future, io, pin::Pin};

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
