
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, ReadHalf, WriteHalf};
use tokio::sync::Mutex as AsyncMutex;
use virtual_mio::InterestHandler;
use wasmer_wasix_types::wasi::Errno;

#[derive(Debug)]
struct PipeState {
    handler: Option<Box<dyn InterestHandler + Send + Sync>>,
}

#[derive(Debug, Clone)]
pub struct PipeRx {
    inner: Arc<AsyncMutex<ReadHalf<DuplexStream>>>,
    state: Arc<Mutex<PipeState>>,
}

#[derive(Debug, Clone)]
pub struct PipeTx {
    inner: Arc<AsyncMutex<WriteHalf<DuplexStream>>>,
    state: Arc<Mutex<PipeState>>,
}

#[derive(Debug, Clone)]
pub struct DuplexPipe {
    inner: Arc<AsyncMutex<DuplexStream>>,
    state: Arc<Mutex<PipeState>>,
}

impl PipeRx {
    pub fn new(inner: ReadHalf<DuplexStream>, state: Arc<Mutex<PipeState>>) -> Self {
        Self {
            inner: Arc::new(AsyncMutex::new(inner)),
            state,
        }
    }

    pub async fn read(&self, buf: &mut [u8]) -> Result<usize, Errno> {
        let mut guard = self.inner.lock().await;
        guard.read(buf).await.map_err(|_| Errno::Io)
    }

    pub fn add_interest_handler(&self, handler: Box<dyn InterestHandler + Send + Sync>) {
        let mut state = self.state.lock().unwrap();
        state.handler = Some(handler);
    }

    pub fn remove_interest_handler(&self) {
        let mut state = self.state.lock().unwrap();
        state.handler = None;
    }
}

impl PipeTx {
    pub fn new(inner: WriteHalf<DuplexStream>, state: Arc<Mutex<PipeState>>) -> Self {
        Self {
            inner: Arc::new(AsyncMutex::new(inner)),
            state,
        }
    }

    pub async fn write(&self, buf: &[u8]) -> Result<usize, Errno> {
        let mut guard = self.inner.lock().await;
        let written = guard.write(buf).await.map_err(|_| Errno::Io)?;
        if let Some(handler) = &mut self.state.lock().unwrap().handler {
            handler.push_interest(virtual_mio::InterestType::Readable);
        }
        Ok(written)
    }

    pub fn add_interest_handler(&self, handler: Box<dyn InterestHandler + Send + Sync>) {
        let mut state = self.state.lock().unwrap();
        state.handler = Some(handler);
    }

    pub fn remove_interest_handler(&self) {
        let mut state = self.state.lock().unwrap();
        state.handler = None;
    }
}

impl DuplexPipe {
    pub fn new(stream: DuplexStream) -> Self {
        Self {
            inner: Arc::new(AsyncMutex::new(stream)),
            state: Arc::new(Mutex::new(PipeState { handler: None })),
        }
    }

    pub fn channel() -> (PipeRx, PipeTx) {
        let (a, b) = tokio::io::duplex(64 * 1024);
        let (rx, tx) = tokio::io::split(a);
        let state = Arc::new(Mutex::new(PipeState { handler: None }));
        (PipeRx::new(rx, state.clone()), PipeTx::new(tx, state))
    }

    pub async fn read(&self, buf: &mut [u8]) -> Result<usize, Errno> {
        let mut guard = self.inner.lock().await;
        guard.read(buf).await.map_err(|_| Errno::Io)
    }

    pub async fn write(&self, buf: &[u8]) -> Result<usize, Errno> {
        let mut guard = self.inner.lock().await;
        let written = guard.write(buf).await.map_err(|_| Errno::Io)?;
        if let Some(handler) = &mut self.state.lock().unwrap().handler {
            handler.push_interest(virtual_mio::InterestType::Readable);
        }
        Ok(written)
    }

    pub fn add_interest_handler(&self, handler: Box<dyn InterestHandler + Send + Sync>) {
        let mut state = self.state.lock().unwrap();
        state.handler = Some(handler);
    }

    pub fn remove_interest_handler(&self) {
        let mut state = self.state.lock().unwrap();
        state.handler = None;
    }
}
