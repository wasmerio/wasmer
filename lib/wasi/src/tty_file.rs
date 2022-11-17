use std::{
    io::{self, *},
    sync::Arc, pin::Pin, task::{Context, Poll}, ops::DerefMut,
};
use wasmer_vfs::{VirtualFile, AsyncSeek, AsyncWrite, AsyncRead};

use crate::runtime::RuntimeStdout;

/// Special file for `/dev/tty` that can print to stdout
/// (hence the requirement for a `WasiRuntimeImplementation`)
#[derive(Debug)]
pub struct TtyFile {
    stdout: RuntimeStdout,
    stdin: Box<dyn VirtualFile + Send + Sync + 'static>,
}

impl TtyFile {
    pub fn new(
        runtime: Arc<dyn crate::WasiRuntimeImplementation + Send + Sync + 'static>,
        stdin: Box<dyn VirtualFile + Send + Sync + 'static>,
    ) -> Self {
        let stdout = RuntimeStdout::new(runtime);
        Self {
            stdout,
            stdin
        }
    }
}

impl AsyncSeek for TtyFile {
    fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> io::Result<()> {
        Ok(())
    }
    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(0))
    }
}

impl AsyncWrite for TtyFile {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let stdout = Pin::new(&mut self.stdout);
        stdout.poll_write(cx, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let stdout = Pin::new(&mut self.stdout);
        stdout.poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let stdout = Pin::new(&mut self.stdout);
        stdout.poll_shutdown(cx)
    }
    fn poll_write_vectored(self: Pin<&mut Self>, cx: &mut Context<'_>, bufs: &[IoSlice<'_>]) -> Poll<io::Result<usize>> {
        let stdout = Pin::new(&mut self.stdout);
        stdout.poll_write_vectored(cx, bufs)
    }
    fn is_write_vectored(&self) -> bool {
        self.stdout.is_write_vectored()
    }
}

impl AsyncRead for TtyFile {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut tokio::io::ReadBuf<'_>) -> Poll<io::Result<()>> {
        let stdin = Pin::new(&mut self.stdin);
        stdin.poll_read(cx, buf)
    }
}

impl VirtualFile for TtyFile {
    fn last_accessed(&self) -> u64 {
        self.stdin.last_accessed()
    }
    fn last_modified(&self) -> u64 {
        self.stdin.last_modified()
    }
    fn created_time(&self) -> u64 {
        self.stdin.created_time()
    }
    fn size(&self) -> u64 {
        0
    }
    fn set_len(&mut self, _new_size: u64) -> wasmer_vfs::Result<()> {
        Ok(())
    }
    fn unlink(&mut self) -> wasmer_vfs::Result<()> {
        Ok(())
    }
    fn is_open(&self) -> bool {
        true
    }
    fn poll_read_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let stdin = Pin::new(self.stdin.deref_mut());
        stdin.poll_read_ready(cx)
    }
    fn poll_write_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let stdout = Pin::new(&mut self.stdout);
        stdout.poll_write_ready(cx)
    }
}

#[cfg(test)]
mod tests {

    use crate::{VirtualNetworking, WasiRuntimeImplementation, WasiEnv};
    use std::{sync::{
        Arc, Mutex,
    }, pin::Pin, io};
    use futures::Future;
    use wasmer_vbus::{DefaultVirtualBus, VirtualBus};
    use wasmer_vfs::{WasiBidirectionalPipePair, AsyncWriteExt};

    struct FakeRuntimeImplementation {
        pub data: Arc<Mutex<Vec<u8>>>,
        pub bus: Arc<dyn VirtualBus<WasiEnv> + Send + Sync + 'static>,
        pub networking: Arc<dyn VirtualNetworking + Send + Sync + 'static>,
    }

    impl Default for FakeRuntimeImplementation {
        fn default() -> Self {
            FakeRuntimeImplementation {
                data: Arc::new(Mutex::new(Vec::new())),
                #[cfg(not(feature = "host-vnet"))]
                networking: Arc::new(wasmer_vnet::UnsupportedVirtualNetworking::default()),
                #[cfg(feature = "host-vnet")]
                networking: Arc::new(wasmer_wasi_local_networking::LocalNetworking::default()),
                bus: Arc::new(DefaultVirtualBus::default()),
            }
        }
    }

    impl FakeRuntimeImplementation {
        fn get_stdout_written(&self) -> Option<Vec<u8>> {
            let s = self.data.try_lock().ok()?;
            Some(s.clone())
        }
    }

    impl std::fmt::Debug for FakeRuntimeImplementation {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "FakeRuntimeImplementation")
        }
    }

    impl WasiRuntimeImplementation for FakeRuntimeImplementation {
        fn bus<'a>(&'a self) -> Arc<dyn VirtualBus<WasiEnv> + Send + Sync + 'static> {
            self.bus.clone()
        }
    
        fn networking<'a>(&'a self) -> Arc<dyn VirtualNetworking + Send + Sync + 'static> {
            self.networking.clone()
        }

        fn stdout(&self, data: &[u8]) -> Pin<Box<dyn Future<Output=io::Result<()>> + Send + Sync>> {
            let inner = self.data.clone();
            Box::pin(async move {
                let mut inner = inner.lock().unwrap();
                inner.extend_from_slice(data);
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn test_tty_file() {
        use crate::tty_file::TtyFile;
        use std::sync::Arc;

        let mut pair = WasiBidirectionalPipePair::new();
        pair.set_blocking(false);

        let rt = Arc::new(FakeRuntimeImplementation::default());
        let mut tty_file = TtyFile::new(rt.clone(), Box::new(pair));
        tty_file.write(b"hello").await.unwrap();
        assert_eq!(rt.get_stdout_written().unwrap(), b"hello".to_vec());
    }
}
