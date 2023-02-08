use std::{
    io::{self, SeekFrom},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use derivative::Derivative;
use futures::Future;
use wasmer_vfs::{AsyncRead, AsyncSeek, AsyncWrite};

#[derive(Derivative)]
#[derivative(Debug)]
pub struct RuntimeStdout {
    runtime: Arc<dyn crate::WasiRuntime + Send + Sync + 'static>,
    #[derivative(Debug = "ignore")]
    writing: Option<StdioState>,
}

/// Holds a future and a pointer to the buffer it is writing.
struct StdioState {
    fut: Pin<Box<dyn Future<Output = io::Result<()>> + Send + Sync + 'static>>,
    buffer_pointer: u64,
}

impl RuntimeStdout {
    pub fn new(runtime: Arc<dyn crate::WasiRuntime + Send + Sync + 'static>) -> Self {
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
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Other,
            "can not seek stdout",
        )))
    }
}

impl AsyncWrite for RuntimeStdout {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let buf_ptr = buf.as_ptr() as u64;
        if let Some(writing) = self.writing.as_mut() {
            if writing.buffer_pointer == buf_ptr {
                let fut = writing.fut.as_mut();
                let written = fut.poll(cx);
                if written.is_ready() {
                    self.writing.take();
                }
                return match written {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                    Poll::Ready(Ok(())) => Poll::Ready(Ok(buf.len())),
                };
            }
        }
        let stdout = self.runtime.stdout(buf);
        self.writing.replace(StdioState {
            fut: stdout,
            buffer_pointer: buf_ptr,
        });
        let writing = self.writing.as_mut().unwrap();
        let fut = writing.fut.as_mut();
        let written = fut.poll(cx);
        if written.is_ready() {
            self.writing.take();
        }
        match written {
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
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
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

    fn poll_read_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(0))
    }

    fn poll_write_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(8192))
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct RuntimeStderr {
    runtime: Arc<dyn crate::WasiRuntime + Send + Sync + 'static>,
    #[derivative(Debug = "ignore")]
    writing: Option<StdioState>,
}

impl RuntimeStderr {
    pub fn new(runtime: Arc<dyn crate::WasiRuntime + Send + Sync + 'static>) -> Self {
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
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Other,
            "can not seek stderr",
        )))
    }
}

impl AsyncWrite for RuntimeStderr {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let buf_ptr = buf.as_ptr() as u64;
        if let Some(state) = self.writing.as_mut() {
            if state.buffer_pointer == buf_ptr {
                let fut = state.fut.as_mut();
                let written = fut.poll(cx);
                if written.is_ready() {
                    self.writing.take();
                }
                return match written {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                    Poll::Ready(Ok(())) => Poll::Ready(Ok(buf.len())),
                };
            }
        }
        let stdout = self.runtime.stdout(buf);
        self.writing.replace(StdioState {
            fut: stdout,
            buffer_pointer: buf_ptr,
        });
        let state = self.writing.as_mut().unwrap();
        let writing = state.fut.as_mut();
        let written = writing.poll(cx);
        if written.is_ready() {
            self.writing.take();
        }
        match written {
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
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
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

    fn poll_read_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(0))
    }

    fn poll_write_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(8192))
    }
}
