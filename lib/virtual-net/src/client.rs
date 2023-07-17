use std::collections::HashMap;
use std::future::Future;
use std::mem::MaybeUninit;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::ops::DerefMut;
use std::pin::Pin;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;
use std::task::RawWaker;
use std::task::RawWakerVTable;
use std::task::Waker;
use std::time::Duration;

use bytes::Buf;
use bytes::BytesMut;
use derivative::Derivative;
use futures_util::future::BoxFuture;
use futures_util::stream::FuturesOrdered;
use futures_util::StreamExt;
use serde::Serialize;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio::io::ReadBuf;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::oneshot;
use wasmer_virtual_io::InlineWaker;
use wasmer_virtual_io::InterestType;

use crate::io_err_into_net_error;
use crate::meta;
use crate::meta::RequestType;
use crate::meta::ResponseType;
use crate::meta::SocketId;
use crate::meta::{MessageRequest, MessageResponse};
use crate::IpCidr;
use crate::IpRoute;
use crate::NetworkError;
use crate::StreamSecurity;
use crate::VirtualConnectedSocket;
use crate::VirtualConnectionlessSocket;
use crate::VirtualIcmpSocket;
use crate::VirtualIoSource;
use crate::VirtualNetworking;
use crate::VirtualRawSocket;
use crate::VirtualSocket;
use crate::VirtualTcpListener;
use crate::VirtualTcpSocket;
use crate::VirtualUdpSocket;

use crate::Result;

#[derive(Debug, Clone)]
pub struct RemoteNetworking {
    common: Arc<RemoteCommon>,
}

impl RemoteNetworking {
    /// Creates a new interface on the remote location using
    /// a unique interface ID and a pair of channels
    pub fn new_from_mpsc(
        tx: mpsc::Sender<MessageRequest>,
        rx: mpsc::Receiver<MessageResponse>,
    ) -> (Self, RemoteNetworkingDriver) {
        let (_, rx_work) = mpsc::unbounded_channel();

        let common = RemoteCommon {
            tx: RemoteTx::Mpsc(tx),
            rx: Mutex::new(RemoteRx::Mpsc(rx)),
            request_seed: AtomicU64::new(1),
            requests: Default::default(),
            socket_seed: AtomicU64::new(1),
            recv_tx: Default::default(),
            recv_with_addr_tx: Default::default(),
            handlers: Default::default(),
            stall: Default::default(),
        };
        let common = Arc::new(common);

        let driver = RemoteNetworkingDriver {
            more_work: rx_work,
            tasks: Default::default(),
            common: common.clone(),
        };
        let networking = Self { common };

        (networking, driver)
    }

    /// Creates a new interface on the remote location using
    /// a unique interface ID and a pair of channels
    pub fn new_from_stream(
        tx: Pin<Box<dyn AsyncWrite + Send + Sync>>,
        rx: Pin<Box<dyn AsyncRead + Send + Sync>>,
    ) -> (Self, RemoteNetworkingDriver) {
        Self::new_from_stream_internal(tx, rx, false)
    }

    /// Creates a new interface on the remote location using
    /// a unique interface ID and a pair of channels
    ///
    /// This version will run the async read and write operations
    /// only the driver (this is needed for mixed runtimes)
    pub fn new_from_stream_via_driver(
        tx: Pin<Box<dyn AsyncWrite + Send + Sync>>,
        rx: Pin<Box<dyn AsyncRead + Send + Sync>>,
    ) -> (Self, RemoteNetworkingDriver) {
        Self::new_from_stream_internal(tx, rx, true)
    }

    fn new_from_stream_internal(
        tx: Pin<Box<dyn AsyncWrite + Send + Sync>>,
        rx: Pin<Box<dyn AsyncRead + Send + Sync>>,
        via_driver: bool,
    ) -> (Self, RemoteNetworkingDriver) {
        let (tx_work, rx_work) = mpsc::unbounded_channel();

        let common = RemoteCommon {
            tx: if via_driver {
                RemoteTx::StreamViaDriver {
                    tx: Arc::new(tokio::sync::Mutex::new(tx)),
                    work: tx_work,
                }
            } else {
                RemoteTx::Stream {
                    tx: tokio::sync::Mutex::new(tx),
                }
            },
            rx: Mutex::new(RemoteRx::Stream {
                rx,
                next: None,
                buf: BytesMut::new(),
            }),
            request_seed: AtomicU64::new(1),
            requests: Default::default(),
            socket_seed: AtomicU64::new(1),
            recv_tx: Default::default(),
            recv_with_addr_tx: Default::default(),
            handlers: Default::default(),
            stall: Default::default(),
        };
        let common = Arc::new(common);

        let driver = RemoteNetworkingDriver {
            more_work: rx_work,
            tasks: Default::default(),
            common: common.clone(),
        };
        let networking = Self { common };

        (networking, driver)
    }

    fn new_socket(&self, id: SocketId) -> RemoteSocket {
        let (tx, rx_recv) = tokio::sync::mpsc::channel(100);
        self.common.recv_tx.lock().unwrap().insert(id, tx);

        let (tx, rx_recv_with_addr) = tokio::sync::mpsc::channel(100);
        self.common.recv_with_addr_tx.lock().unwrap().insert(id, tx);

        RemoteSocket {
            socket_id: id,
            common: self.common.clone(),
            rx_buffer: BytesMut::new(),
            rx_recv,
            rx_recv_with_addr,
            tx_waker: TxWaker::new(&self.common).as_waker(),
        }
    }
}

pin_project_lite::pin_project! {
    pub struct RemoteNetworkingDriver {
        common: Arc<RemoteCommon>,
        more_work: mpsc::UnboundedReceiver<BoxFuture<'static, ()>>,
        #[pin]
        tasks: FuturesOrdered<BoxFuture<'static, ()>>,
    }
}

impl Future for RemoteNetworkingDriver {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // This guard will be held while the pipeline is not currently
        // stalled by some back pressure. It is only acquired when there
        // is background tasks being processed
        let mut not_stalled_guard = None;

        // We loop until the waker is registered with the receiving stream
        // and all the background tasks
        loop {
            // Background tasks are sent to this driver in certain circumstances
            while let Poll::Ready(Some(work)) = Pin::new(&mut self.more_work).poll_recv(cx) {
                self.tasks.push_back(work);
            }

            // Background work basically stalls the stream until its all processed
            // which makes the back pressure system work properly
            match self.tasks.poll_next_unpin(cx) {
                Poll::Ready(Some(_)) => continue,
                Poll::Ready(None) => {
                    not_stalled_guard.take();
                }
                Poll::Pending if not_stalled_guard.is_none() => {
                    if let Ok(guard) = self.common.stall.clone().try_lock_owned() {
                        not_stalled_guard.replace(guard);
                    } else {
                        return Poll::Pending;
                    }
                }
                Poll::Pending => {}
            };

            // We grab the next message sent by the server to us
            let msg = {
                let mut rx_guard = self.common.rx.lock().unwrap();
                rx_guard.poll(cx)
            };
            return match msg {
                Poll::Ready(Some(msg)) => {
                    match msg {
                        MessageResponse::Recv { socket_id, data } => {
                            let tx = {
                                let guard = self.common.recv_tx.lock().unwrap();
                                match guard.get(&socket_id) {
                                    Some(tx) => tx.clone(),
                                    None => continue,
                                }
                            };
                            let common = self.common.clone();
                            self.tasks.push_back(Box::pin(async move {
                                tx.send(data).await.ok();

                                common
                                    .handlers
                                    .lock()
                                    .unwrap()
                                    .get_mut(&socket_id)
                                    .map(|h| h.interest(InterestType::Readable));
                            }));
                        }
                        MessageResponse::RecvWithAddr {
                            socket_id,
                            data,
                            addr,
                        } => {
                            let tx = {
                                let guard = self.common.recv_with_addr_tx.lock().unwrap();
                                match guard.get(&socket_id) {
                                    Some(tx) => tx.clone(),
                                    None => continue,
                                }
                            };
                            let common = self.common.clone();
                            self.tasks.push_back(Box::pin(async move {
                                tx.send((data, addr)).await.ok();

                                common
                                    .handlers
                                    .lock()
                                    .unwrap()
                                    .get_mut(&socket_id)
                                    .map(|h| h.interest(InterestType::Readable));
                            }));
                        }
                        MessageResponse::Sent { socket_id, .. } => {
                            self.common
                                .handlers
                                .lock()
                                .unwrap()
                                .get_mut(&socket_id)
                                .map(|h| h.interest(InterestType::Writable));
                        }
                        MessageResponse::SendError {
                            socket_id, error, ..
                        } => match &error {
                            NetworkError::ConnectionAborted
                            | NetworkError::ConnectionReset
                            | NetworkError::BrokenPipe => {
                                self.common
                                    .handlers
                                    .lock()
                                    .unwrap()
                                    .get_mut(&socket_id)
                                    .map(|h| h.interest(InterestType::Closed));
                            }
                            _ => {
                                self.common
                                    .handlers
                                    .lock()
                                    .unwrap()
                                    .get_mut(&socket_id)
                                    .map(|h| h.interest(InterestType::Writable));
                            }
                        },
                        MessageResponse::Closed { socket_id } => {
                            self.common
                                .handlers
                                .lock()
                                .unwrap()
                                .get_mut(&socket_id)
                                .map(|h| h.interest(InterestType::Closed));
                        }
                        MessageResponse::ResponseToRequest { req_id, res } => {
                            let mut requests = self.common.requests.lock().unwrap();
                            if let Some(request) = requests.remove(&req_id) {
                                request.try_send(res).ok();
                            }
                        }
                    }
                    continue;
                }
                Poll::Ready(None) => Poll::Ready(()),
                Poll::Pending => Poll::Pending,
            };
        }
    }
}

pub(crate) type RemoteTxStream = Pin<Box<dyn AsyncWrite + Send + Sync>>;
pub(crate) enum RemoteTx<T>
where
    T: Serialize,
{
    Mpsc(mpsc::Sender<T>),
    Stream {
        tx: tokio::sync::Mutex<RemoteTxStream>,
    },
    StreamViaDriver {
        tx: Arc<tokio::sync::Mutex<RemoteTxStream>>,
        work: mpsc::UnboundedSender<BoxFuture<'static, ()>>,
    },
}
impl<T> RemoteTx<T>
where
    T: Serialize + Send + Sync + 'static,
{
    pub(crate) async fn send(&self, req: T) -> Result<()> {
        match self {
            RemoteTx::Mpsc(tx) => tx
                .send(req)
                .await
                .map_err(|_| NetworkError::ConnectionAborted),
            RemoteTx::Stream { tx, .. } => {
                let mut tx = tx.lock().await;
                let data = bincode::serialize(&req).map_err(|err| {
                    tracing::warn!("failed to serialize message - {}", err);
                    NetworkError::IOError
                })?;
                let data_len = data.len() as u64;
                let data_len_buf = data_len.to_le_bytes();
                tx.write_all(&data_len_buf)
                    .await
                    .map_err(io_err_into_net_error)?;
                tx.write_all(&data).await.map_err(io_err_into_net_error)
            }
            RemoteTx::StreamViaDriver { tx, work, .. } => {
                let (tx_done, rx_done) = oneshot::channel();
                let tx = tx.clone();
                work.send(Box::pin(async move {
                    let job = async {
                        let mut tx = tx.lock().await;
                        let data = bincode::serialize(&req).map_err(|err| {
                            tracing::warn!("failed to serialize message - {}", err);
                            NetworkError::IOError
                        })?;
                        let data_len = data.len() as u64;
                        let data_len_buf = data_len.to_le_bytes();
                        tx.write_all(&data_len_buf)
                            .await
                            .map_err(io_err_into_net_error)?;
                        tx.write_all(&data).await.map_err(io_err_into_net_error)
                    };
                    tx_done.send(job.await).ok();
                }))
                .map_err(|_| NetworkError::ConnectionAborted)?;

                rx_done
                    .await
                    .unwrap_or_else(|_| Err(NetworkError::ConnectionAborted))
            }
        }
    }
    pub(crate) fn try_send(&self, interest_cx: &mut Context<'_>, req: T) -> Result<()> {
        match self {
            RemoteTx::Mpsc(tx) => match tx.try_send(req) {
                Ok(()) => Ok(()),
                Err(TrySendError::Closed(_)) => Err(NetworkError::ConnectionAborted),
                Err(TrySendError::Full(_)) => Err(NetworkError::WouldBlock),
            },
            RemoteTx::Stream { tx } => {
                let data = bincode::serialize(&req).map_err(|err| {
                    tracing::warn!("failed to serialize message - {}", err);
                    NetworkError::IOError
                })?;
                let data_len = data.len() as u64;
                let data_len_buf = data_len.to_le_bytes();

                let mut tx = InlineWaker::block_on(tx.lock());
                let data_len_buf_left =
                    match Pin::new(tx.deref_mut()).poll_write(interest_cx, &data_len_buf) {
                        Poll::Ready(Ok(0)) => {
                            return Err(NetworkError::ConnectionAborted);
                        }
                        Poll::Ready(Ok(amt)) if amt == data_len_buf.len() => 0,
                        Poll::Ready(Ok(amt)) => data_len_buf.len() - amt,
                        Poll::Ready(Err(err)) => {
                            return Err(io_err_into_net_error(err));
                        }
                        Poll::Pending => {
                            return Err(NetworkError::WouldBlock);
                        }
                    };

                InlineWaker::block_on(Box::pin(async move {
                    if data_len_buf_left > 0 {
                        let offset = data_len_buf.len() - data_len_buf_left;
                        tx.write_all(&data_len_buf[offset..])
                            .await
                            .map_err(io_err_into_net_error)?;
                    }
                    tx.write_all(&data).await.map_err(io_err_into_net_error)
                }))
            }
            RemoteTx::StreamViaDriver { tx, work } => {
                let data = bincode::serialize(&req).map_err(|err| {
                    tracing::warn!("failed to serialize message - {}", err);
                    NetworkError::IOError
                })?;
                let data_len = data.len() as u64;
                let data_len_buf = data_len.to_le_bytes();

                let interest_waker = interest_cx.waker().clone();

                let (tx_done, rx_done) = std::sync::mpsc::channel();
                let tx = tx.clone();
                work.send(Box::pin(async move {
                    let job = async {
                        let mut tx = tx.lock().await;

                        let mut interest_cx = Context::from_waker(&interest_waker);
                        let data_len_buf_left = match Pin::new(tx.deref_mut())
                            .poll_write(&mut interest_cx, &data_len_buf)
                        {
                            Poll::Ready(Ok(0)) => {
                                return Err(NetworkError::ConnectionAborted);
                            }
                            Poll::Ready(Ok(amt)) if amt == data_len_buf.len() => 0,
                            Poll::Ready(Ok(amt)) => data_len_buf.len() - amt,
                            Poll::Ready(Err(err)) => {
                                return Err(io_err_into_net_error(err));
                            }
                            Poll::Pending => {
                                return Err(NetworkError::WouldBlock);
                            }
                        };

                        if data_len_buf_left > 0 {
                            let offset = data_len_buf.len() - data_len_buf_left;
                            tx.write_all(&data_len_buf[offset..])
                                .await
                                .map_err(io_err_into_net_error)?;
                        }
                        tx.write_all(&data).await.map_err(io_err_into_net_error)
                    };
                    let ret = job.await;
                    tx_done.send(ret).ok();
                }))
                .map_err(|_| NetworkError::ConnectionAborted)?;

                rx_done
                    .recv()
                    .unwrap_or_else(|_| Err(NetworkError::ConnectionAborted))
            }
        }
    }
}

#[derive(Debug)]
struct TxWaker {
    common: Arc<RemoteCommon>,
}
impl TxWaker {
    pub fn new(common: &Arc<RemoteCommon>) -> Arc<Self> {
        Arc::new(Self {
            common: common.clone(),
        })
    }

    fn wake_now(&self) {
        let mut guard = self.common.handlers.lock().unwrap();
        for (_, handler) in guard.iter_mut() {
            handler.interest(InterestType::Writable);
        }
    }

    pub fn as_waker(self: &Arc<Self>) -> Waker {
        let s: *const Self = Arc::into_raw(Arc::clone(self));
        let raw_waker = RawWaker::new(s as *const (), &VTABLE);
        unsafe { Waker::from_raw(raw_waker) }
    }
}

fn tx_waker_wake(s: &TxWaker) {
    let waker_arc = unsafe { Arc::from_raw(s) };
    waker_arc.wake_now();
}

fn tx_waker_clone(s: &TxWaker) -> RawWaker {
    let arc = unsafe { Arc::from_raw(s) };
    std::mem::forget(arc.clone());
    RawWaker::new(Arc::into_raw(arc) as *const (), &VTABLE)
}

const VTABLE: RawWakerVTable = unsafe {
    RawWakerVTable::new(
        |s| tx_waker_clone(&*(s as *const TxWaker)),  // clone
        |s| tx_waker_wake(&*(s as *const TxWaker)),   // wake
        |s| (*(s as *const TxWaker)).wake_now(),      // wake by ref (don't decrease refcount)
        |s| drop(Arc::from_raw(s as *const TxWaker)), // decrease refcount
    )
};

pub(crate) enum RemoteRx<T>
where
    T: serde::de::DeserializeOwned,
{
    Mpsc(mpsc::Receiver<T>),
    Stream {
        rx: Pin<Box<dyn AsyncRead + Send + Sync>>,
        next: Option<u64>,
        buf: BytesMut,
    },
}
impl<T> RemoteRx<T>
where
    T: serde::de::DeserializeOwned,
{
    pub(crate) fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Option<T>> {
        loop {
            return match self {
                RemoteRx::Mpsc(rx) => Pin::new(rx).poll_recv(cx),
                RemoteRx::Stream { rx, next, buf } => {
                    match next.clone() {
                        Some(next) if (buf.len() as u64) >= next => {
                            let next = next as usize;
                            let msg = match bincode::deserialize(&buf[..next]) {
                                Ok(m) => m,
                                Err(err) => {
                                    tracing::warn!("failed to deserialize message - {}", err);
                                    return Poll::Ready(None);
                                }
                            };
                            buf.advance(next);
                            if buf.is_empty() {
                                buf.clear();
                            }
                            return Poll::Ready(Some(msg));
                        }
                        None if buf.len() >= 8 => {
                            let mut data_len_buf = [0u8; 8];
                            data_len_buf.copy_from_slice(&buf[..8]);
                            buf.advance(8);
                            if buf.is_empty() {
                                buf.clear();
                            }
                            next.replace(u64::from_le_bytes(data_len_buf));
                            continue;
                        }
                        _ => {}
                    }

                    let mut chunk: [MaybeUninit<u8>; 10240] =
                        unsafe { MaybeUninit::uninit().assume_init() };
                    let chunk_unsafe: &mut [MaybeUninit<u8>] = &mut chunk[..];
                    let chunk_unsafe: &mut [u8] = unsafe { std::mem::transmute(chunk_unsafe) };

                    let mut read_buf = ReadBuf::new(chunk_unsafe);
                    match rx.as_mut().poll_read(cx, &mut read_buf) {
                        Poll::Ready(Ok(_)) => {
                            let filled = read_buf.filled();
                            if filled.is_empty() {
                                return Poll::Ready(None);
                            }
                            buf.extend_from_slice(&filled);
                            continue;
                        }
                        Poll::Ready(Err(err)) => {
                            tracing::warn!("failed to read from channel - {}", err);
                            Poll::Ready(None)
                        }
                        Poll::Pending => Poll::Pending,
                    }
                }
            };
        }
    }
}

#[derive(Debug)]
struct RequestTx {
    tx: mpsc::Sender<ResponseType>,
}
impl RequestTx {
    pub fn try_send(self, msg: ResponseType) -> Result<()> {
        match self.tx.try_send(msg) {
            Ok(()) => Ok(()),
            Err(TrySendError::Closed(_)) => Err(NetworkError::ConnectionAborted),
            Err(TrySendError::Full(_)) => Err(NetworkError::WouldBlock),
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
struct RemoteCommon {
    #[derivative(Debug = "ignore")]
    tx: RemoteTx<MessageRequest>,
    #[derivative(Debug = "ignore")]
    rx: Mutex<RemoteRx<MessageResponse>>,
    request_seed: AtomicU64,
    requests: Mutex<HashMap<u64, RequestTx>>,
    socket_seed: AtomicU64,
    recv_tx: Mutex<HashMap<SocketId, mpsc::Sender<Vec<u8>>>>,
    recv_with_addr_tx: Mutex<HashMap<SocketId, mpsc::Sender<(Vec<u8>, SocketAddr)>>>,
    #[derivative(Debug = "ignore")]
    handlers: Mutex<HashMap<SocketId, Box<dyn wasmer_virtual_io::InterestHandler + Send + Sync>>>,

    // The stall guard will prevent reads while its held and there are background tasks running
    // (the idea behind this is to create back pressure so that the task list infinitely grow)
    stall: Arc<tokio::sync::Mutex<()>>,
}

impl RemoteCommon {
    async fn io_iface(&self, req: RequestType) -> ResponseType {
        let req_id = self.request_seed.fetch_add(1, Ordering::SeqCst);
        let mut req_rx = {
            let (tx, rx) = mpsc::channel(1);
            let mut guard = self.requests.lock().unwrap();
            guard.insert(req_id, RequestTx { tx });
            rx
        };
        if let Err(err) = self
            .tx
            .send(MessageRequest::Interface { req_id, req })
            .await
        {
            return ResponseType::Err(err);
        };
        req_rx.recv().await.unwrap()
    }

    fn blocking_io_iface(&self, req: RequestType) -> ResponseType {
        InlineWaker::block_on(self.io_iface(req))
    }
}

#[async_trait::async_trait]
impl VirtualNetworking for RemoteNetworking {
    async fn bridge(
        &self,
        network: &str,
        access_token: &str,
        security: StreamSecurity,
    ) -> Result<()> {
        match self
            .common
            .io_iface(RequestType::Bridge {
                network: network.to_string(),
                access_token: access_token.to_string(),
                security,
            })
            .await
        {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    async fn unbridge(&self) -> Result<()> {
        match self.common.io_iface(RequestType::Unbridge).await {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    async fn dhcp_acquire(&self) -> Result<Vec<IpAddr>> {
        match self.common.io_iface(RequestType::DhcpAcquire).await {
            ResponseType::Err(err) => Err(err),
            ResponseType::IpAddressList(ips) => Ok(ips),
            _ => Err(NetworkError::IOError),
        }
    }

    fn ip_add(&self, ip: IpAddr, prefix: u8) -> Result<()> {
        match self
            .common
            .blocking_io_iface(RequestType::IpAdd { ip, prefix })
        {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn ip_remove(&self, ip: IpAddr) -> Result<()> {
        match self.common.blocking_io_iface(RequestType::IpRemove(ip)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn ip_clear(&self) -> Result<()> {
        match self.common.blocking_io_iface(RequestType::IpClear) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn ip_list(&self) -> Result<Vec<IpCidr>> {
        match self.common.blocking_io_iface(RequestType::Unbridge) {
            ResponseType::Err(err) => Err(err),
            ResponseType::CidrList(routes) => Ok(routes),
            _ => Err(NetworkError::IOError),
        }
    }

    fn mac(&self) -> Result<[u8; 6]> {
        match self.common.blocking_io_iface(RequestType::GetMac) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Mac(mac) => Ok(mac),
            _ => Err(NetworkError::IOError),
        }
    }

    fn gateway_set(&self, ip: IpAddr) -> Result<()> {
        match self.common.blocking_io_iface(RequestType::GatewaySet(ip)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn route_add(
        &self,
        cidr: IpCidr,
        via_router: IpAddr,
        preferred_until: Option<Duration>,
        expires_at: Option<Duration>,
    ) -> Result<()> {
        match self.common.blocking_io_iface(RequestType::RouteAdd {
            cidr,
            via_router,
            preferred_until,
            expires_at,
        }) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn route_remove(&self, cidr: IpAddr) -> Result<()> {
        match self
            .common
            .blocking_io_iface(RequestType::RouteRemove(cidr))
        {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn route_clear(&self) -> Result<()> {
        match self.common.blocking_io_iface(RequestType::RouteClear) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn route_list(&self) -> Result<Vec<IpRoute>> {
        match self.common.blocking_io_iface(RequestType::GetRouteList) {
            ResponseType::Err(err) => Err(err),
            ResponseType::RouteList(routes) => Ok(routes),
            _ => Err(NetworkError::IOError),
        }
    }

    async fn bind_raw(&self) -> Result<Box<dyn VirtualRawSocket + Sync>> {
        let socket_id: SocketId = self
            .common
            .socket_seed
            .fetch_add(1, Ordering::SeqCst)
            .into();
        match self.common.io_iface(RequestType::BindRaw(socket_id)).await {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(Box::new(self.new_socket(socket_id))),
            _ => Err(NetworkError::IOError),
        }
    }

    async fn listen_tcp(
        &self,
        addr: SocketAddr,
        only_v6: bool,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> Result<Box<dyn VirtualTcpListener + Sync>> {
        let socket_id: SocketId = self
            .common
            .socket_seed
            .fetch_add(1, Ordering::SeqCst)
            .into();
        match self
            .common
            .io_iface(RequestType::ListenTcp {
                socket_id,
                addr,
                only_v6,
                reuse_port,
                reuse_addr,
            })
            .await
        {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(Box::new(self.new_socket(socket_id))),
            _ => Err(NetworkError::IOError),
        }
    }

    async fn bind_udp(
        &self,
        addr: SocketAddr,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> Result<Box<dyn VirtualUdpSocket + Sync>> {
        let socket_id: SocketId = self
            .common
            .socket_seed
            .fetch_add(1, Ordering::SeqCst)
            .into();
        match self
            .common
            .io_iface(RequestType::BindUdp {
                socket_id,
                addr,
                reuse_port,
                reuse_addr,
            })
            .await
        {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(Box::new(self.new_socket(socket_id))),
            _ => Err(NetworkError::IOError),
        }
    }

    async fn bind_icmp(&self, addr: IpAddr) -> Result<Box<dyn VirtualIcmpSocket + Sync>> {
        let socket_id: SocketId = self
            .common
            .socket_seed
            .fetch_add(1, Ordering::SeqCst)
            .into();
        match self
            .common
            .io_iface(RequestType::BindIcmp { socket_id, addr })
            .await
        {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(Box::new(self.new_socket(socket_id))),
            _ => Err(NetworkError::IOError),
        }
    }

    async fn connect_tcp(
        &self,
        addr: SocketAddr,
        peer: SocketAddr,
    ) -> Result<Box<dyn VirtualTcpSocket + Sync>> {
        let socket_id: SocketId = self
            .common
            .socket_seed
            .fetch_add(1, Ordering::SeqCst)
            .into();
        match self
            .common
            .io_iface(RequestType::ConnectTcp {
                socket_id,
                addr,
                peer,
            })
            .await
        {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(Box::new(self.new_socket(socket_id))),
            _ => Err(NetworkError::IOError),
        }
    }

    async fn resolve(
        &self,
        host: &str,
        port: Option<u16>,
        dns_server: Option<IpAddr>,
    ) -> Result<Vec<IpAddr>> {
        match self
            .common
            .io_iface(RequestType::Resolve {
                host: host.to_string(),
                port,
                dns_server,
            })
            .await
        {
            ResponseType::Err(err) => Err(err),
            ResponseType::IpAddressList(ips) => Ok(ips),
            _ => Err(NetworkError::IOError),
        }
    }
}

#[derive(Debug)]
struct RemoteSocket {
    socket_id: SocketId,
    common: Arc<RemoteCommon>,
    rx_buffer: BytesMut,
    rx_recv: mpsc::Receiver<Vec<u8>>,
    rx_recv_with_addr: mpsc::Receiver<(Vec<u8>, SocketAddr)>,
    tx_waker: Waker,
}
impl Drop for RemoteSocket {
    fn drop(&mut self) {
        self.common.recv_tx.lock().unwrap().remove(&self.socket_id);
        self.common
            .recv_with_addr_tx
            .lock()
            .unwrap()
            .remove(&self.socket_id);
    }
}

impl RemoteSocket {
    async fn io_socket(&self, req: RequestType) -> ResponseType {
        let req_id = self.common.request_seed.fetch_add(1, Ordering::SeqCst);
        let mut req_rx = {
            let (tx, rx) = mpsc::channel(1);
            let mut guard = self.common.requests.lock().unwrap();
            guard.insert(req_id, RequestTx { tx });
            rx
        };
        if let Err(err) = self
            .common
            .tx
            .send(MessageRequest::Socket {
                socket: self.socket_id,
                req_id,
                req,
            })
            .await
        {
            return ResponseType::Err(err);
        };
        req_rx.recv().await.unwrap()
    }

    fn blocking_io_socket(&self, req: RequestType) -> ResponseType {
        InlineWaker::block_on(self.io_socket(req))
    }
}

impl VirtualIoSource for RemoteSocket {
    fn remove_handler(&mut self) {
        self.common.handlers.lock().unwrap().remove(&self.socket_id);
    }
}

impl VirtualSocket for RemoteSocket {
    fn set_ttl(&mut self, ttl: u32) -> Result<()> {
        match self.blocking_io_socket(RequestType::SetTtl(ttl)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn ttl(&self) -> Result<u32> {
        match self.blocking_io_socket(RequestType::GetTtl) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Ttl(ttl) => Ok(ttl),
            _ => Err(NetworkError::IOError),
        }
    }

    fn addr_local(&self) -> Result<SocketAddr> {
        match self.blocking_io_socket(RequestType::GetAddrLocal) {
            ResponseType::Err(err) => Err(err),
            ResponseType::SocketAddr(addr) => Ok(addr),
            _ => Err(NetworkError::IOError),
        }
    }

    fn status(&self) -> Result<crate::SocketStatus> {
        match self.blocking_io_socket(RequestType::GetStatus) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Status(status) => Ok(status),
            _ => Err(NetworkError::IOError),
        }
    }

    fn set_handler(
        &mut self,
        handler: Box<dyn wasmer_virtual_io::InterestHandler + Send + Sync>,
    ) -> Result<()> {
        self.common
            .handlers
            .lock()
            .unwrap()
            .insert(self.socket_id, handler);
        Ok(())
    }
}

impl VirtualTcpListener for RemoteSocket {
    fn try_accept(&mut self) -> Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)> {
        let child_id: SocketId = self
            .common
            .socket_seed
            .fetch_add(1, Ordering::SeqCst)
            .into();
        match self.blocking_io_socket(RequestType::TryAccept(child_id)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::SocketWithAddr { id, addr } => {
                let (tx, rx_recv) = tokio::sync::mpsc::channel(100);
                self.common.recv_tx.lock().unwrap().insert(id, tx);

                let (tx, rx_recv_with_addr) = tokio::sync::mpsc::channel(100);
                self.common.recv_with_addr_tx.lock().unwrap().insert(id, tx);

                let socket = RemoteSocket {
                    socket_id: id,
                    common: self.common.clone(),
                    rx_buffer: BytesMut::new(),
                    rx_recv,
                    rx_recv_with_addr,
                    tx_waker: TxWaker::new(&self.common).as_waker(),
                };
                Ok((Box::new(socket), addr))
            }
            _ => Err(NetworkError::IOError),
        }
    }

    fn set_handler(
        &mut self,
        handler: Box<dyn wasmer_virtual_io::InterestHandler + Send + Sync>,
    ) -> Result<()> {
        VirtualSocket::set_handler(self, handler)
    }

    fn addr_local(&self) -> Result<SocketAddr> {
        match self.blocking_io_socket(RequestType::GetAddrLocal) {
            ResponseType::Err(err) => Err(err),
            ResponseType::SocketAddr(addr) => Ok(addr),
            _ => Err(NetworkError::IOError),
        }
    }

    fn set_ttl(&mut self, ttl: u8) -> Result<()> {
        match self.blocking_io_socket(RequestType::SetTtl(ttl as u32)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn ttl(&self) -> Result<u8> {
        match self.blocking_io_socket(RequestType::GetTtl) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Ttl(val) => Ok(val.try_into().map_err(|_| NetworkError::InvalidData)?),
            _ => Err(NetworkError::IOError),
        }
    }
}

impl VirtualRawSocket for RemoteSocket {
    fn try_send(&mut self, data: &[u8]) -> Result<usize> {
        let req_id = self.common.request_seed.fetch_add(1, Ordering::SeqCst);

        let mut cx = Context::from_waker(&self.tx_waker);
        self.common
            .tx
            .try_send(
                &mut cx,
                MessageRequest::Send {
                    socket: self.socket_id,
                    data: data.to_vec(),
                    req_id,
                },
            )
            .map(|_| data.len())
    }

    fn try_flush(&mut self) -> Result<()> {
        match self.blocking_io_socket(RequestType::Flush) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn try_recv(&mut self, buf: &mut [std::mem::MaybeUninit<u8>]) -> Result<usize> {
        loop {
            if self.rx_buffer.len() > 0 {
                let amt = self.rx_buffer.len().min(buf.len());
                let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
                buf.copy_from_slice(&self.rx_buffer[..amt]);
                self.rx_buffer.advance(amt);
                return Ok(amt);
            }
            match self.rx_recv.try_recv() {
                Ok(data) => self.rx_buffer.extend_from_slice(&data),
                Err(TryRecvError::Disconnected) => return Err(NetworkError::ConnectionAborted),
                Err(TryRecvError::Empty) => return Err(NetworkError::WouldBlock),
            }
        }
    }

    fn set_promiscuous(&mut self, promiscuous: bool) -> Result<()> {
        match self.blocking_io_socket(RequestType::SetPromiscuous(promiscuous)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn promiscuous(&self) -> Result<bool> {
        match self.blocking_io_socket(RequestType::GetPromiscuous) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Flag(val) => Ok(val),
            _ => Err(NetworkError::IOError),
        }
    }
}

impl VirtualConnectionlessSocket for RemoteSocket {
    fn try_send_to(&mut self, data: &[u8], addr: SocketAddr) -> Result<usize> {
        let req_id = self.common.request_seed.fetch_add(1, Ordering::SeqCst);
        let mut cx = Context::from_waker(&self.tx_waker);
        self.common
            .tx
            .try_send(
                &mut cx,
                MessageRequest::SendTo {
                    socket: self.socket_id,
                    data: data.to_vec(),
                    addr,
                    req_id,
                },
            )
            .map(|_| data.len())
    }

    fn try_recv_from(
        &mut self,
        buf: &mut [std::mem::MaybeUninit<u8>],
    ) -> Result<(usize, SocketAddr)> {
        match self.rx_recv_with_addr.try_recv() {
            Ok((data, addr)) => {
                let amt = buf.len().min(data.len());
                let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
                buf.copy_from_slice(&data[..amt]);
                Ok((amt, addr))
            }
            Err(TryRecvError::Disconnected) => Err(NetworkError::ConnectionAborted),
            Err(TryRecvError::Empty) => Err(NetworkError::WouldBlock),
        }
    }
}

impl VirtualUdpSocket for RemoteSocket {
    fn set_broadcast(&mut self, broadcast: bool) -> Result<()> {
        match self.blocking_io_socket(RequestType::SetBroadcast(broadcast)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn broadcast(&self) -> Result<bool> {
        match self.blocking_io_socket(RequestType::GetBroadcast) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Flag(val) => Ok(val),
            _ => Err(NetworkError::IOError),
        }
    }

    fn set_multicast_loop_v4(&mut self, val: bool) -> Result<()> {
        match self.blocking_io_socket(RequestType::SetMulticastLoopV4(val)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn multicast_loop_v4(&self) -> Result<bool> {
        match self.blocking_io_socket(RequestType::GetMulticastLoopV4) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Flag(val) => Ok(val),
            _ => Err(NetworkError::IOError),
        }
    }

    fn set_multicast_loop_v6(&mut self, val: bool) -> Result<()> {
        match self.blocking_io_socket(RequestType::SetMulticastLoopV6(val)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn multicast_loop_v6(&self) -> Result<bool> {
        match self.blocking_io_socket(RequestType::GetMulticastLoopV6) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Flag(val) => Ok(val),
            _ => Err(NetworkError::IOError),
        }
    }

    fn set_multicast_ttl_v4(&mut self, ttl: u32) -> Result<()> {
        match self.blocking_io_socket(RequestType::SetMulticastTtlV4(ttl)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn multicast_ttl_v4(&self) -> Result<u32> {
        match self.blocking_io_socket(RequestType::GetMulticastTtlV4) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Ttl(ttl) => Ok(ttl),
            _ => Err(NetworkError::IOError),
        }
    }

    fn join_multicast_v4(
        &mut self,
        multiaddr: std::net::Ipv4Addr,
        iface: std::net::Ipv4Addr,
    ) -> Result<()> {
        match self.blocking_io_socket(RequestType::JoinMulticastV4 { multiaddr, iface }) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn leave_multicast_v4(
        &mut self,
        multiaddr: std::net::Ipv4Addr,
        iface: std::net::Ipv4Addr,
    ) -> Result<()> {
        match self.blocking_io_socket(RequestType::LeaveMulticastV4 { multiaddr, iface }) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn join_multicast_v6(&mut self, multiaddr: std::net::Ipv6Addr, iface: u32) -> Result<()> {
        match self.blocking_io_socket(RequestType::JoinMulticastV6 { multiaddr, iface }) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn leave_multicast_v6(&mut self, multiaddr: std::net::Ipv6Addr, iface: u32) -> Result<()> {
        match self.blocking_io_socket(RequestType::LeaveMulticastV6 { multiaddr, iface }) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn addr_peer(&self) -> Result<Option<SocketAddr>> {
        match self.blocking_io_socket(RequestType::GetAddrPeer) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(None),
            ResponseType::SocketAddr(addr) => Ok(Some(addr)),
            _ => Err(NetworkError::IOError),
        }
    }
}

impl VirtualIcmpSocket for RemoteSocket {}

impl VirtualConnectedSocket for RemoteSocket {
    fn set_linger(&mut self, linger: Option<Duration>) -> Result<()> {
        match self.blocking_io_socket(RequestType::SetLinger(linger)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn linger(&self) -> Result<Option<Duration>> {
        match self.blocking_io_socket(RequestType::GetLinger) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(None),
            ResponseType::Duration(val) => Ok(Some(val)),
            _ => Err(NetworkError::IOError),
        }
    }

    fn try_send(&mut self, data: &[u8]) -> Result<usize> {
        let req_id = self.common.request_seed.fetch_add(1, Ordering::SeqCst);
        let mut cx = Context::from_waker(&self.tx_waker);
        self.common
            .tx
            .try_send(
                &mut cx,
                MessageRequest::Send {
                    socket: self.socket_id,
                    data: data.to_vec(),
                    req_id,
                },
            )
            .map(|_| data.len())
    }

    fn try_flush(&mut self) -> Result<()> {
        match self.blocking_io_socket(RequestType::Flush) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn close(&mut self) -> Result<()> {
        match self.blocking_io_socket(RequestType::Close) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn try_recv(&mut self, buf: &mut [std::mem::MaybeUninit<u8>]) -> Result<usize> {
        loop {
            if self.rx_buffer.len() > 0 {
                let amt = self.rx_buffer.len().min(buf.len());
                let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
                buf.copy_from_slice(&self.rx_buffer[..amt]);
                self.rx_buffer.advance(amt);
                return Ok(amt);
            }
            match self.rx_recv.try_recv() {
                Ok(data) => self.rx_buffer.extend_from_slice(&data),
                Err(TryRecvError::Disconnected) => return Err(NetworkError::ConnectionAborted),
                Err(TryRecvError::Empty) => return Err(NetworkError::WouldBlock),
            }
        }
    }
}

impl VirtualTcpSocket for RemoteSocket {
    fn set_recv_buf_size(&mut self, size: usize) -> Result<()> {
        match self.blocking_io_socket(RequestType::SetRecvBufSize(size as u64)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn recv_buf_size(&self) -> Result<usize> {
        match self.blocking_io_socket(RequestType::GetRecvBufSize) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Amount(amt) => Ok(amt.try_into().map_err(|_| NetworkError::IOError)?),
            _ => Err(NetworkError::IOError),
        }
    }

    fn set_send_buf_size(&mut self, size: usize) -> Result<()> {
        match self.blocking_io_socket(RequestType::SetSendBufSize(size as u64)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn send_buf_size(&self) -> Result<usize> {
        match self.blocking_io_socket(RequestType::GetSendBufSize) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Amount(val) => Ok(val.try_into().map_err(|_| NetworkError::IOError)?),
            _ => Err(NetworkError::IOError),
        }
    }

    fn set_nodelay(&mut self, reuse: bool) -> Result<()> {
        match self.blocking_io_socket(RequestType::SetNoDelay(reuse)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn nodelay(&self) -> Result<bool> {
        match self.blocking_io_socket(RequestType::GetNoDelay) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Flag(val) => Ok(val),
            _ => Err(NetworkError::IOError),
        }
    }

    fn addr_peer(&self) -> Result<SocketAddr> {
        match self.blocking_io_socket(RequestType::GetAddrPeer) {
            ResponseType::Err(err) => Err(err),
            ResponseType::SocketAddr(addr) => Ok(addr),
            _ => Err(NetworkError::IOError),
        }
    }

    fn shutdown(&mut self, how: std::net::Shutdown) -> Result<()> {
        let shutdown = match how {
            std::net::Shutdown::Read => meta::Shutdown::Read,
            std::net::Shutdown::Write => meta::Shutdown::Write,
            std::net::Shutdown::Both => meta::Shutdown::Both,
        };
        match self.blocking_io_socket(RequestType::Shutdown(shutdown)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn is_closed(&self) -> bool {
        match self.blocking_io_socket(RequestType::IsClosed) {
            ResponseType::Flag(val) => val,
            _ => false,
        }
    }
}
