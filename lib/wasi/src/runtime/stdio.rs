use std::io::{self, SeekFrom};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Poll, Context};

use derivative::Derivative;
use futures::Future;
use wasmer_vfs::{AsyncRead, AsyncWrite, AsyncSeek};

#[derive(Derivative)]
#[derivative(Debug)]
pub struct RuntimeStdout {
    runtime: Arc<dyn crate::WasiRuntimeImplementation + Send + Sync + 'static>,
    #[derivative(Debug = "ignore")]
    writing: Option<(Pin<Box<dyn Future<Output=io::Result<()>> + Send + Sync + 'static>>, u64)>,
}

impl RuntimeStdout {
    pub fn new(runtime: Arc<dyn crate::WasiRuntimeImplementation + Send + Sync + 'static>) -> Self {
        Self {
            runtime,
            writing: None,
        }
    }
}

impl AsyncSeek for RuntimeStdout {
    fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stdout"))
    }
    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "can not seek stdout")))
    }
}

impl AsyncWrite for RuntimeStdout {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let buf_ptr = buf.as_ptr() as u64;
        if let Some((writing, buf2)) = self.writing.as_mut() {
            if *buf2 == buf_ptr {
                let writing = writing.as_mut();
                return match writing.poll(cx) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                    Poll::Ready(Ok(())) => Poll::Ready(Ok(buf.len())),
                };
            }
        }
        self.writing.replace(
            (self.runtime.stdout(buf), buf_ptr)
        );
        let (writing, _) = self.writing.as_mut().unwrap();
        let writing = writing.as_mut();
        match writing.poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Ready(Ok(())) => Poll::Ready(Ok(buf.len())),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for RuntimeStdout {
    fn poll_read(self: Pin<&mut Self>, _cx: &mut Context<'_>, _buf: &mut tokio::io::ReadBuf<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stdout",
        )))
    }
}

impl wasmer_vfs::VirtualFile for RuntimeStdout {
    fn last_accessed(&self) -> u64 {
        0
    }

    fn last_modified(&self) -> u64 {
        0
    }

    fn created_time(&self) -> u64 {
        0
    }

    fn size(&self) -> u64 {
        0
    }

    fn set_len(&mut self, _new_size: u64) -> wasmer_vfs::Result<()> {
        tracing::debug!("Calling VirtualFile::set_len on stderr; this is probably a bug");
        Err(wasmer_vfs::FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> wasmer_vfs::Result<()> {
        Ok(())
    }

    fn poll_read_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(0))
    }

    fn poll_write_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(8192))
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct RuntimeStderr {
    runtime: Arc<dyn crate::WasiRuntimeImplementation + Send + Sync + 'static>,
    #[derivative(Debug = "ignore")]
    writing: Option<(Pin<Box<dyn Future<Output=io::Result<()>> + Send + Sync + 'static>>, u64)>,
}

impl RuntimeStderr {
    pub fn new(runtime: Arc<dyn crate::WasiRuntimeImplementation + Send + Sync + 'static>) -> Self {
        Self {
            runtime,
            writing: None,
        }
    }
}

impl AsyncSeek for RuntimeStderr {
    fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stderr"))
    }
    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "can not seek stderr")))
    }
}

impl AsyncWrite for RuntimeStderr {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let buf_ptr = buf.as_ptr() as u64;
        if let Some((writing, buf2)) = self.writing.as_mut() {
            if *buf2 == buf_ptr {
                let writing = writing.as_mut();
                return match writing.poll(cx) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                    Poll::Ready(Ok(())) => Poll::Ready(Ok(buf.len())),
                };
            }
        }
        self.writing.replace(
            (self.runtime.stdout(buf), buf_ptr)
        );
        let (writing, _) = self.writing.as_mut().unwrap();
        let writing = writing.as_mut();
        match writing.poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Ready(Ok(())) => Poll::Ready(Ok(buf.len())),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for RuntimeStderr {
    fn poll_read(self: Pin<&mut Self>, _cx: &mut Context<'_>, _buf: &mut tokio::io::ReadBuf<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stderr",
        )))
    }
}


impl wasmer_vfs::VirtualFile for RuntimeStderr {
    fn last_accessed(&self) -> u64 {
        0
    }

    fn last_modified(&self) -> u64 {
        0
    }

    fn created_time(&self) -> u64 {
        0
    }

    fn size(&self) -> u64 {
        0
    }

    fn set_len(&mut self, _new_size: u64) -> wasmer_vfs::Result<()> {
        tracing::debug!("Calling VirtualFile::set_len on stderr; this is probably a bug");
        Err(wasmer_vfs::FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> wasmer_vfs::Result<()> {
        Ok(())
    }

    fn poll_read_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(0))
    }

    fn poll_write_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(8192))
    }
}
