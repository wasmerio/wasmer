use std::{
    io,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::Mutex,
};

use crate::{ByteSink, ByteSource, Host, LocalFuture, Stdio};

/// The host on a tokio runtime with timers enabled: system clocks, tokio timers and the OS
/// entropy source.
pub struct TokioHost {
    start: Instant,
}

impl Default for TokioHost {
    fn default() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Host for TokioHost {
    fn realtime(&self) -> Duration {
        // A host clock before 1970 is reported as the epoch.
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
    }

    fn monotonic(&self) -> Duration {
        self.start.elapsed()
    }

    fn sleep(&self, duration: Duration) -> LocalFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }

    fn random(&self, buf: &mut [u8]) {
        getrandom::fill(buf).expect("the host entropy source failed");
    }
}

impl Stdio {
    /// The host process's own stdin, stdout and stderr, through tokio.
    ///
    /// tokio reads stdin with a blocking read on its blocking pool that cannot be cancelled, so a
    /// guest blocked in `read(0, ..)` can delay runtime shutdown until input arrives.
    pub fn inherit() -> Self {
        Self {
            stdin: Arc::new(HostStream(Mutex::new(tokio::io::stdin()))),
            stdout: Arc::new(HostStream(Mutex::new(tokio::io::stdout()))),
            stderr: Arc::new(HostStream(Mutex::new(tokio::io::stderr()))),
        }
    }
}

struct HostStream<T>(Mutex<T>);

impl<T: AsyncRead + Unpin + Send> ByteSource for HostStream<T> {
    fn read<'a>(&'a self, buf: &'a mut [u8]) -> LocalFuture<'a, io::Result<usize>> {
        Box::pin(async move { self.0.lock().await.read(buf).await })
    }
}

impl<T: AsyncWrite + Unpin + Send> ByteSink for HostStream<T> {
    // Flushed per call, so output reaches the host in the order the guest wrote it.
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        Box::pin(async move {
            let mut stream = self.0.lock().await;
            let n = stream.write(data).await?;
            stream.flush().await?;
            Ok(n)
        })
    }
}
