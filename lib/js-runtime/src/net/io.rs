use std::borrow::Cow;
use std::net::SocketAddr;
use std::rc::Rc;
use std::sync::Arc;

use deno_core::AsyncMutFuture;
use deno_core::AsyncRefCell;
use deno_core::AsyncResult;
use deno_core::CancelHandle;
use deno_core::CancelTryFuture;
use deno_core::RcRef;
use deno_core::Resource;
use deno_core::futures::TryFutureExt;
use deno_error::JsErrorBox;
use parking_lot::Mutex;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio::io::{ReadHalf, WriteHalf};
use virtual_net::{VirtualTcpSocket, net_error_into_io_err};

use super::stream::SharedTcpStream;

#[derive(Debug)]
pub struct FullDuplexResource<R, W> {
    rd: AsyncRefCell<R>,
    wr: AsyncRefCell<W>,
    cancel_handle: CancelHandle,
}

impl<R, W> FullDuplexResource<R, W>
where
    R: AsyncRead + Unpin + 'static,
    W: AsyncWrite + Unpin + 'static,
{
    pub fn new((rd, wr): (R, W)) -> Self {
        Self {
            rd: rd.into(),
            wr: wr.into(),
            cancel_handle: Default::default(),
        }
    }

    pub fn into_inner(self) -> (R, W) {
        (self.rd.into_inner(), self.wr.into_inner())
    }

    pub fn rd_borrow_mut(self: &Rc<Self>) -> AsyncMutFuture<R> {
        RcRef::map(self, |r| &r.rd).borrow_mut()
    }

    pub fn wr_borrow_mut(self: &Rc<Self>) -> AsyncMutFuture<W> {
        RcRef::map(self, |r| &r.wr).borrow_mut()
    }

    pub fn cancel_handle(self: &Rc<Self>) -> RcRef<CancelHandle> {
        RcRef::map(self, |r| &r.cancel_handle)
    }

    pub fn cancel_read_ops(&self) {
        self.cancel_handle.cancel()
    }

    pub async fn read(self: Rc<Self>, data: &mut [u8]) -> Result<usize, std::io::Error> {
        let mut rd = self.rd_borrow_mut().await;
        let nread = rd.read(data).try_or_cancel(self.cancel_handle()).await?;
        Ok(nread)
    }

    pub async fn write(self: Rc<Self>, data: &[u8]) -> Result<usize, std::io::Error> {
        let mut wr = self.wr_borrow_mut().await;
        let nwritten = wr.write(data).await?;
        Ok(nwritten)
    }

    pub async fn shutdown(self: Rc<Self>) -> Result<(), std::io::Error> {
        let mut wr = self.wr_borrow_mut().await;
        wr.shutdown().await?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error, deno_error::JsError)]
pub enum MapError {
    #[class(inherit)]
    #[error("{0}")]
    Io(std::io::Error),
    #[class(generic)]
    #[error("Unable to get resources")]
    NoResources,
}

#[derive(Debug)]
pub struct TcpStreamResource {
    inner: FullDuplexResource<ReadHalf<SharedTcpStream>, WriteHalf<SharedTcpStream>>,
    socket: Arc<Mutex<Box<dyn VirtualTcpSocket + Sync>>>,
}

impl TcpStreamResource {
    pub fn new(
        stream: SharedTcpStream,
        socket: Arc<Mutex<Box<dyn VirtualTcpSocket + Sync>>>,
    ) -> Self {
        let (rd, wr) = tokio::io::split(stream);
        Self {
            inner: FullDuplexResource::new((rd, wr)),
            socket,
        }
    }

    pub fn set_nodelay(self: Rc<Self>, nodelay: bool) -> Result<(), MapError> {
        self.map_socket(|socket| socket.set_nodelay(nodelay))
    }

    pub fn set_keepalive(self: Rc<Self>, keepalive: bool) -> Result<(), MapError> {
        self.map_socket(|socket| socket.set_keepalive(keepalive))
    }

    pub fn set_dontroute(self: Rc<Self>, dontroute: bool) -> Result<(), MapError> {
        self.map_socket(|socket| socket.set_dontroute(dontroute))
    }

    pub fn local_addr(&self) -> Result<SocketAddr, MapError> {
        let mut socket = self.socket.lock();
        socket
            .addr_local()
            .map_err(net_error_into_io_err)
            .map_err(MapError::Io)
    }

    pub fn remote_addr(&self) -> Result<SocketAddr, MapError> {
        let mut socket = self.socket.lock();
        socket
            .addr_peer()
            .map_err(net_error_into_io_err)
            .map_err(MapError::Io)
    }

    pub fn into_stream(self) -> SharedTcpStream {
        let (rd, wr) = self.inner.into_inner();
        rd.unsplit(wr)
    }

    pub async fn read(self: Rc<Self>, data: &mut [u8]) -> Result<usize, std::io::Error> {
        self.inner.read(data).await
    }

    pub async fn write(self: Rc<Self>, data: &[u8]) -> Result<usize, std::io::Error> {
        self.inner.write(data).await
    }

    fn map_socket(
        self: Rc<Self>,
        map: impl FnOnce(&mut dyn VirtualTcpSocket) -> Result<(), virtual_net::NetworkError>,
    ) -> Result<(), MapError> {
        let mut socket = self.socket.lock();
        map(socket.as_mut())
            .map_err(net_error_into_io_err)
            .map_err(MapError::Io)
    }
}

impl Resource for TcpStreamResource {
    deno_core::impl_readable_byob!();
    deno_core::impl_writable!();

    fn name(&self) -> Cow<'_, str> {
        "tcpStream".into()
    }

    fn read(self: Rc<Self>, limit: usize) -> AsyncResult<deno_core::BufView> {
        let mut buf = vec![0u8; limit];
        Box::pin(async move {
            let nread = self
                .inner
                .read(&mut buf)
                .await
                .map_err(JsErrorBox::from_err)?;
            buf.truncate(nread);
            Ok(buf.into())
        })
    }

    fn shutdown(self: Rc<Self>) -> AsyncResult<()> {
        Box::pin(self.inner.shutdown().map_err(JsErrorBox::from_err))
    }

    fn close(self: Rc<Self>) {
        self.inner.cancel_read_ops();
    }
}
