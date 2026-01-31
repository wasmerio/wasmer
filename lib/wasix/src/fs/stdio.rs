use std::sync::{Arc, Mutex};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex as AsyncMutex;
use virtual_mio::InterestHandler;
use wasmer_wasix_types::wasi::Errno;

#[derive(Debug, Clone)]
pub struct Stdio {
    inner: Arc<StdioInner>,
}

#[derive(Debug)]
struct StdioInner {
    reader: Option<AsyncMutex<Box<dyn AsyncRead + Send + Unpin>>>,
    writer: Option<AsyncMutex<Box<dyn AsyncWrite + Send + Unpin>>>,
    handler: Mutex<Option<Box<dyn InterestHandler + Send + Sync>>>,
}

impl Stdio {
    pub fn stdin() -> Self {
        Self {
            inner: Arc::new(StdioInner {
                reader: Some(AsyncMutex::new(Box::new(tokio::io::stdin()))),
                writer: None,
                handler: Mutex::new(None),
            }),
        }
    }

    pub fn stdout() -> Self {
        Self {
            inner: Arc::new(StdioInner {
                reader: None,
                writer: Some(AsyncMutex::new(Box::new(tokio::io::stdout()))),
                handler: Mutex::new(None),
            }),
        }
    }

    pub fn stderr() -> Self {
        Self {
            inner: Arc::new(StdioInner {
                reader: None,
                writer: Some(AsyncMutex::new(Box::new(tokio::io::stderr()))),
                handler: Mutex::new(None),
            }),
        }
    }

    pub fn from_reader(reader: Box<dyn AsyncRead + Send + Unpin>) -> Self {
        Self {
            inner: Arc::new(StdioInner {
                reader: Some(AsyncMutex::new(reader)),
                writer: None,
                handler: Mutex::new(None),
            }),
        }
    }

    pub fn from_writer(writer: Box<dyn AsyncWrite + Send + Unpin>) -> Self {
        Self {
            inner: Arc::new(StdioInner {
                reader: None,
                writer: Some(AsyncMutex::new(writer)),
                handler: Mutex::new(None),
            }),
        }
    }

    pub fn from_rw(
        reader: Box<dyn AsyncRead + Send + Unpin>,
        writer: Box<dyn AsyncWrite + Send + Unpin>,
    ) -> Self {
        Self {
            inner: Arc::new(StdioInner {
                reader: Some(AsyncMutex::new(reader)),
                writer: Some(AsyncMutex::new(writer)),
                handler: Mutex::new(None),
            }),
        }
    }

    pub async fn read(&self, buf: &mut [u8]) -> Result<usize, Errno> {
        let Some(reader) = &self.inner.reader else {
            return Err(Errno::Badf);
        };
        let mut guard = reader.lock().await;
        guard.read(buf).await.map_err(|_| Errno::Io)
    }

    pub async fn write(&self, buf: &[u8]) -> Result<usize, Errno> {
        let Some(writer) = &self.inner.writer else {
            return Err(Errno::Badf);
        };
        let mut guard = writer.lock().await;
        guard.write(buf).await.map_err(|_| Errno::Io)
    }

    pub fn add_interest_handler(&self, handler: Box<dyn InterestHandler + Send + Sync>) {
        let mut guard = self.inner.handler.lock().unwrap();
        *guard = Some(handler);
    }

    pub fn remove_interest_handler(&self) {
        let mut guard = self.inner.handler.lock().unwrap();
        *guard = None;
    }
}

pub type Stdin = Stdio;
pub type Stdout = Stdio;
pub type Stderr = Stdio;
