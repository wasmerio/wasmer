use std::collections::HashMap;
use std::collections::VecDeque;
use std::future::Future;
use std::net::IpAddr;
use std::net::SocketAddr;
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
use futures_util::future::BoxFuture;
use futures_util::stream::FuturesOrdered;
use futures_util::Sink;
use futures_util::Stream;
use futures_util::StreamExt;
#[cfg(feature = "hyper")]
use hyper_util::rt::tokio::TokioIo;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::error::TrySendError;
use tokio_serde::formats::SymmetricalBincode;
#[cfg(feature = "cbor")]
use tokio_serde::formats::SymmetricalCbor;
#[cfg(feature = "json")]
use tokio_serde::formats::SymmetricalJson;
#[cfg(feature = "messagepack")]
use tokio_serde::formats::SymmetricalMessagePack;
use tokio_serde::SymmetricallyFramed;
use tokio_util::codec::FramedRead;
use tokio_util::codec::FramedWrite;
use tokio_util::codec::LengthDelimitedCodec;
use virtual_mio::InlineWaker;
use virtual_mio::InterestType;

use crate::meta;
use crate::meta::FrameSerializationFormat;
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

use crate::rx_tx::RemoteRx;
use crate::rx_tx::RemoteTx;
use crate::rx_tx::RemoteTxWakers;
use crate::Result;

#[derive(Debug, Clone)]
pub struct RemoteNetworkingClient {
    common: Arc<RemoteCommon>,
}

impl RemoteNetworkingClient {
    fn new(
        tx: RemoteTx<MessageRequest>,
        rx: RemoteRx<MessageResponse>,
        rx_work: mpsc::UnboundedReceiver<BoxFuture<'static, ()>>,
    ) -> (Self, RemoteNetworkingClientDriver) {
        let common = RemoteCommon {
            tx,
            rx: Mutex::new(rx),
            request_seed: AtomicU64::new(1),
            requests: Default::default(),
            socket_seed: AtomicU64::new(1),
            recv_tx: Default::default(),
            recv_with_addr_tx: Default::default(),
            accept_tx: Default::default(),
            sent_tx: Default::default(),
            handlers: Default::default(),
            stall: Default::default(),
        };
        let common = Arc::new(common);

        let driver = RemoteNetworkingClientDriver {
            more_work: rx_work,
            tasks: Default::default(),
            common: common.clone(),
        };
        let networking = Self { common };

        (networking, driver)
    }

    /// Creates a new interface on the remote location using
    /// a unique interface ID and a pair of channels
    pub fn new_from_mpsc(
        tx: mpsc::Sender<MessageRequest>,
        rx: mpsc::Receiver<MessageResponse>,
    ) -> (Self, RemoteNetworkingClientDriver) {
        let (tx_work, rx_work) = mpsc::unbounded_channel();
        let tx_wakers = RemoteTxWakers::default();

        let tx = RemoteTx::Mpsc {
            tx,
            work: tx_work,
            wakers: tx_wakers.clone(),
        };
        let rx = RemoteRx::Mpsc {
            rx,
            wakers: tx_wakers,
        };

        Self::new(tx, rx, rx_work)
    }

    /// Creates a new interface on the remote location using
    /// a unique interface ID and a pair of channels
    ///
    /// This version will run the async read and write operations
    /// only the driver (this is needed for mixed runtimes)
    pub fn new_from_async_io<TX, RX>(
        tx: TX,
        rx: RX,
        format: FrameSerializationFormat,
    ) -> (Self, RemoteNetworkingClientDriver)
    where
        TX: AsyncWrite + Send + 'static,
        RX: AsyncRead + Send + 'static,
    {
        let tx = FramedWrite::new(tx, LengthDelimitedCodec::new());
        let tx: Pin<Box<dyn Sink<MessageRequest, Error = std::io::Error> + Send + 'static>> =
            match format {
                FrameSerializationFormat::Bincode => {
                    Box::pin(SymmetricallyFramed::new(tx, SymmetricalBincode::default()))
                }
                #[cfg(feature = "json")]
                FrameSerializationFormat::Json => {
                    Box::pin(SymmetricallyFramed::new(tx, SymmetricalJson::default()))
                }
                #[cfg(feature = "messagepack")]
                FrameSerializationFormat::MessagePack => Box::pin(SymmetricallyFramed::new(
                    tx,
                    SymmetricalMessagePack::default(),
                )),
                #[cfg(feature = "cbor")]
                FrameSerializationFormat::Cbor => {
                    Box::pin(SymmetricallyFramed::new(tx, SymmetricalCbor::default()))
                }
            };

        let rx = FramedRead::new(rx, LengthDelimitedCodec::new());
        let rx: Pin<Box<dyn Stream<Item = std::io::Result<MessageResponse>> + Send + 'static>> =
            match format {
                FrameSerializationFormat::Bincode => {
                    Box::pin(SymmetricallyFramed::new(rx, SymmetricalBincode::default()))
                }
                #[cfg(feature = "json")]
                FrameSerializationFormat::Json => {
                    Box::pin(SymmetricallyFramed::new(rx, SymmetricalJson::default()))
                }
                #[cfg(feature = "messagepack")]
                FrameSerializationFormat::MessagePack => Box::pin(SymmetricallyFramed::new(
                    rx,
                    SymmetricalMessagePack::default(),
                )),
                #[cfg(feature = "cbor")]
                FrameSerializationFormat::Cbor => {
                    Box::pin(SymmetricallyFramed::new(rx, SymmetricalCbor::default()))
                }
            };

        let (tx_work, rx_work) = mpsc::unbounded_channel();
        let tx_wakers = RemoteTxWakers::default();

        let tx = RemoteTx::Stream {
            tx: Arc::new(tokio::sync::Mutex::new(tx)),
            work: tx_work,
            wakers: tx_wakers,
        };
        let rx = RemoteRx::Stream { rx };

        Self::new(tx, rx, rx_work)
    }

    /// Creates a new interface on the remote location using
    /// a unique interface ID and a pair of channels
    #[cfg(feature = "hyper")]
    pub fn new_from_hyper_ws_io(
        tx: futures_util::stream::SplitSink<
            hyper_tungstenite::WebSocketStream<TokioIo<hyper::upgrade::Upgraded>>,
            hyper_tungstenite::tungstenite::Message,
        >,
        rx: futures_util::stream::SplitStream<
            hyper_tungstenite::WebSocketStream<TokioIo<hyper::upgrade::Upgraded>>,
        >,
        format: FrameSerializationFormat,
    ) -> (Self, RemoteNetworkingClientDriver) {
        let (tx_work, rx_work) = mpsc::unbounded_channel();

        let tx = RemoteTx::HyperWebSocket {
            tx: Arc::new(tokio::sync::Mutex::new(tx)),
            work: tx_work,
            wakers: RemoteTxWakers::default(),
            format,
        };
        let rx = RemoteRx::HyperWebSocket { rx, format };
        Self::new(tx, rx, rx_work)
    }

    /// Creates a new interface on the remote location using
    /// a unique interface ID and a pair of channels
    #[cfg(feature = "tokio-tungstenite")]
    pub fn new_from_tokio_ws_io(
        tx: futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            tokio_tungstenite::tungstenite::Message,
        >,
        rx: futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
        format: FrameSerializationFormat,
    ) -> (Self, RemoteNetworkingClientDriver) {
        let (tx_work, rx_work) = mpsc::unbounded_channel();

        let tx = RemoteTx::TokioWebSocket {
            tx: Arc::new(tokio::sync::Mutex::new(tx)),
            work: tx_work,
            wakers: RemoteTxWakers::default(),
            format,
        };
        let rx = RemoteRx::TokioWebSocket { rx, format };
        Self::new(tx, rx, rx_work)
    }

    fn new_socket(&self, id: SocketId) -> RemoteSocket {
        let (tx, rx_recv) = tokio::sync::mpsc::channel(100);
        self.common.recv_tx.lock().unwrap().insert(id, tx);

        let (tx, rx_recv_with_addr) = tokio::sync::mpsc::channel(100);
        self.common.recv_with_addr_tx.lock().unwrap().insert(id, tx);

        let (tx, rx_accept) = tokio::sync::mpsc::channel(100);
        self.common.accept_tx.lock().unwrap().insert(id, tx);

        let (tx, rx_sent) = tokio::sync::mpsc::channel(100);
        self.common.sent_tx.lock().unwrap().insert(id, tx);

        RemoteSocket {
            socket_id: id,
            common: self.common.clone(),
            rx_buffer: BytesMut::new(),
            rx_recv,
            rx_recv_with_addr,
            rx_accept,
            rx_sent,
            tx_waker: TxWaker::new(&self.common).as_waker(),
            pending_accept: None,
            buffer_accept: Default::default(),
            buffer_recv_with_addr: Default::default(),
            send_available: 0,
        }
    }
}

pin_project_lite::pin_project! {
    pub struct RemoteNetworkingClientDriver {
        common: Arc<RemoteCommon>,
        more_work: mpsc::UnboundedReceiver<BoxFuture<'static, ()>>,
        #[pin]
        tasks: FuturesOrdered<BoxFuture<'static, ()>>,
    }
}

impl Future for RemoteNetworkingClientDriver {
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
                                    None => {
                                        continue;
                                    }
                                }
                            };
                            let common = self.common.clone();
                            self.tasks.push_back(Box::pin(async move {
                                tx.send(data).await.ok();

                                if let Some(h) = common.handlers.lock().unwrap().get_mut(&socket_id)
                                {
                                    h.push_interest(InterestType::Readable)
                                }
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
                                tx.send(DataWithAddr { data, addr }).await.ok();

                                if let Some(h) = common.handlers.lock().unwrap().get_mut(&socket_id)
                                {
                                    h.push_interest(InterestType::Readable)
                                }
                            }));
                        }
                        MessageResponse::Sent {
                            socket_id, amount, ..
                        } => {
                            let tx = {
                                let guard = self.common.sent_tx.lock().unwrap();
                                match guard.get(&socket_id) {
                                    Some(tx) => tx.clone(),
                                    None => continue,
                                }
                            };
                            self.tasks.push_back(Box::pin(async move {
                                tx.send(amount).await.ok();
                            }));
                            if let Some(h) =
                                self.common.handlers.lock().unwrap().get_mut(&socket_id)
                            {
                                h.push_interest(InterestType::Writable)
                            }
                        }
                        MessageResponse::SendError {
                            socket_id, error, ..
                        } => match &error {
                            NetworkError::ConnectionAborted
                            | NetworkError::ConnectionReset
                            | NetworkError::BrokenPipe => {
                                if let Some(h) =
                                    self.common.handlers.lock().unwrap().get_mut(&socket_id)
                                {
                                    h.push_interest(InterestType::Closed)
                                }
                            }
                            _ => {
                                if let Some(h) =
                                    self.common.handlers.lock().unwrap().get_mut(&socket_id)
                                {
                                    h.push_interest(InterestType::Writable)
                                }
                            }
                        },
                        MessageResponse::FinishAccept {
                            socket_id,
                            child_id,
                            addr,
                        } => {
                            let common = self.common.clone();
                            self.tasks.push_back(Box::pin(async move {
                                let tx = common.accept_tx.lock().unwrap().get(&socket_id).cloned();
                                if let Some(tx) = tx {
                                    tx.send(SocketWithAddr {
                                        socket: child_id,
                                        addr,
                                    })
                                    .await
                                    .ok();
                                }

                                if let Some(h) = common.handlers.lock().unwrap().get_mut(&socket_id)
                                {
                                    h.push_interest(InterestType::Readable)
                                }
                            }));
                        }
                        MessageResponse::Closed { socket_id } => {
                            if let Some(h) =
                                self.common.handlers.lock().unwrap().get_mut(&socket_id)
                            {
                                h.push_interest(InterestType::Closed)
                            }
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
            handler.push_interest(InterestType::Writable);
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

#[derive(Debug)]
struct DataWithAddr {
    pub data: Vec<u8>,
    pub addr: SocketAddr,
}
#[derive(Debug)]
struct SocketWithAddr {
    pub socket: SocketId,
    pub addr: SocketAddr,
}
type SocketMap<T> = HashMap<SocketId, T>;

#[derive(derive_more::Debug)]
struct RemoteCommon {
    #[debug(ignore)]
    tx: RemoteTx<MessageRequest>,
    #[debug(ignore)]
    rx: Mutex<RemoteRx<MessageResponse>>,
    request_seed: AtomicU64,
    requests: Mutex<HashMap<u64, RequestTx>>,
    socket_seed: AtomicU64,
    recv_tx: Mutex<SocketMap<mpsc::Sender<Vec<u8>>>>,
    recv_with_addr_tx: Mutex<SocketMap<mpsc::Sender<DataWithAddr>>>,
    accept_tx: Mutex<SocketMap<mpsc::Sender<SocketWithAddr>>>,
    sent_tx: Mutex<SocketMap<mpsc::Sender<u64>>>,
    #[debug(ignore)]
    handlers: Mutex<SocketMap<Box<dyn virtual_mio::InterestHandler + Send + Sync>>>,

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
            .send(MessageRequest::Interface {
                req_id: Some(req_id),
                req,
            })
            .await
        {
            return ResponseType::Err(err);
        };
        req_rx.recv().await.unwrap()
    }

    fn io_iface_fire_and_forget(&self, req: RequestType) -> Result<()> {
        self.tx
            .send_with_driver(MessageRequest::Interface { req_id: None, req })
    }
}

#[async_trait::async_trait]
impl VirtualNetworking for RemoteNetworkingClient {
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
            res => {
                tracing::debug!("invalid response to bridge request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    async fn unbridge(&self) -> Result<()> {
        match self.common.io_iface(RequestType::Unbridge).await {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            res => {
                tracing::debug!("invalid response to unbridge request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    async fn dhcp_acquire(&self) -> Result<Vec<IpAddr>> {
        match self.common.io_iface(RequestType::DhcpAcquire).await {
            ResponseType::Err(err) => Err(err),
            ResponseType::IpAddressList(ips) => Ok(ips),
            res => {
                tracing::debug!("invalid response to DHCP acquire request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    async fn ip_add(&self, ip: IpAddr, prefix: u8) -> Result<()> {
        self.common
            .io_iface_fire_and_forget(RequestType::IpAdd { ip, prefix })
    }

    async fn ip_remove(&self, ip: IpAddr) -> Result<()> {
        self.common
            .io_iface_fire_and_forget(RequestType::IpRemove(ip))
    }

    async fn ip_clear(&self) -> Result<()> {
        self.common.io_iface_fire_and_forget(RequestType::IpClear)
    }

    async fn ip_list(&self) -> Result<Vec<IpCidr>> {
        match self.common.io_iface(RequestType::GetIpList).await {
            ResponseType::Err(err) => Err(err),
            ResponseType::CidrList(routes) => Ok(routes),
            res => {
                tracing::debug!("invalid response to IP list request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    async fn mac(&self) -> Result<[u8; 6]> {
        match self.common.io_iface(RequestType::GetMac).await {
            ResponseType::Err(err) => Err(err),
            ResponseType::Mac(mac) => Ok(mac),
            res => {
                tracing::debug!("invalid response to MAC request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    async fn gateway_set(&self, ip: IpAddr) -> Result<()> {
        self.common
            .io_iface_fire_and_forget(RequestType::GatewaySet(ip))
    }

    async fn route_add(
        &self,
        cidr: IpCidr,
        via_router: IpAddr,
        preferred_until: Option<Duration>,
        expires_at: Option<Duration>,
    ) -> Result<()> {
        self.common.io_iface_fire_and_forget(RequestType::RouteAdd {
            cidr,
            via_router,
            preferred_until,
            expires_at,
        })
    }

    async fn route_remove(&self, cidr: IpAddr) -> Result<()> {
        self.common
            .io_iface_fire_and_forget(RequestType::RouteRemove(cidr))
    }

    async fn route_clear(&self) -> Result<()> {
        self.common
            .io_iface_fire_and_forget(RequestType::RouteClear)
    }

    async fn route_list(&self) -> Result<Vec<IpRoute>> {
        match self.common.io_iface(RequestType::GetRouteList).await {
            ResponseType::Err(err) => Err(err),
            ResponseType::RouteList(routes) => Ok(routes),
            res => {
                tracing::debug!("invalid response to route list request - {res:?}");
                Err(NetworkError::IOError)
            }
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
            ResponseType::Socket(socket_id) => Ok(Box::new(self.new_socket(socket_id))),
            res => {
                tracing::debug!("invalid response to bind RAw request - {res:?}");
                Err(NetworkError::IOError)
            }
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
            ResponseType::Socket(socket_id) => {
                let mut socket = self.new_socket(socket_id);
                socket.touch_begin_accept().ok();
                Ok(Box::new(socket))
            }
            res => {
                tracing::debug!("invalid response to listen TCP request - {res:?}");
                Err(NetworkError::IOError)
            }
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
            ResponseType::Socket(socket_id) => Ok(Box::new(self.new_socket(socket_id))),
            res => {
                tracing::debug!("invalid response to bind UDP request - {res:?}");
                Err(NetworkError::IOError)
            }
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
            ResponseType::Socket(socket_id) => Ok(Box::new(self.new_socket(socket_id))),
            res => {
                tracing::debug!("invalid response to bind ICMP request - {res:?}");
                Err(NetworkError::IOError)
            }
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
            ResponseType::Socket(socket_id) => Ok(Box::new(self.new_socket(socket_id))),
            res => {
                tracing::debug!("invalid response to connect TCP request - {res:?}");
                Err(NetworkError::IOError)
            }
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
            res => {
                tracing::debug!("invalid response to resolve request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }
}

#[derive(Debug)]
struct RemoteSocket {
    socket_id: SocketId,
    common: Arc<RemoteCommon>,
    rx_buffer: BytesMut,
    rx_recv: mpsc::Receiver<Vec<u8>>,
    rx_recv_with_addr: mpsc::Receiver<DataWithAddr>,
    tx_waker: Waker,
    rx_accept: mpsc::Receiver<SocketWithAddr>,
    rx_sent: mpsc::Receiver<u64>,
    pending_accept: Option<(SocketId, mpsc::Receiver<Vec<u8>>)>,
    buffer_recv_with_addr: VecDeque<DataWithAddr>,
    buffer_accept: VecDeque<SocketWithAddr>,
    send_available: u64,
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
                req_id: Some(req_id),
                req,
            })
            .await
        {
            return ResponseType::Err(err);
        };
        req_rx.recv().await.unwrap()
    }

    fn io_socket_fire_and_forget(&self, req: RequestType) -> Result<()> {
        self.common.tx.send_with_driver(MessageRequest::Socket {
            socket: self.socket_id,
            req_id: None,
            req,
        })
    }

    fn touch_begin_accept(&mut self) -> Result<()> {
        if self.pending_accept.is_some() {
            return Ok(());
        }
        let child_id: SocketId = self
            .common
            .socket_seed
            .fetch_add(1, Ordering::SeqCst)
            .into();
        self.io_socket_fire_and_forget(RequestType::BeginAccept(child_id))?;

        let (tx, rx_recv) = tokio::sync::mpsc::channel(100);
        self.common.recv_tx.lock().unwrap().insert(child_id, tx);

        self.pending_accept.replace((child_id, rx_recv));
        Ok(())
    }
}

impl VirtualIoSource for RemoteSocket {
    fn remove_handler(&mut self) {
        self.common.handlers.lock().unwrap().remove(&self.socket_id);
    }

    fn poll_read_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<usize>> {
        if !self.rx_buffer.is_empty() {
            return Poll::Ready(Ok(self.rx_buffer.len()));
        }
        match self.rx_recv.poll_recv(cx) {
            Poll::Ready(Some(data)) => {
                self.rx_buffer.extend_from_slice(&data);
                return Poll::Ready(Ok(self.rx_buffer.len()));
            }
            Poll::Ready(None) => return Poll::Ready(Ok(0)),
            Poll::Pending => {}
        }
        if !self.buffer_recv_with_addr.is_empty() {
            let total = self
                .buffer_recv_with_addr
                .iter()
                .map(|a| a.data.len())
                .sum();
            return Poll::Ready(Ok(total));
        }
        match self.rx_recv_with_addr.poll_recv(cx) {
            Poll::Ready(Some(data)) => self.buffer_recv_with_addr.push_back(data),
            Poll::Ready(None) => return Poll::Ready(Ok(0)),
            Poll::Pending => {}
        }
        if !self.buffer_accept.is_empty() {
            return Poll::Ready(Ok(self.buffer_accept.len()));
        }
        match self.rx_accept.poll_recv(cx) {
            Poll::Ready(Some(data)) => self.buffer_accept.push_back(data),
            Poll::Ready(None) => {}
            Poll::Pending => {}
        }
        Poll::Pending
    }

    fn poll_write_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<usize>> {
        if self.send_available > 0 {
            return Poll::Ready(Ok(self.send_available as usize));
        }
        match self.rx_sent.poll_recv(cx) {
            Poll::Ready(Some(amt)) => {
                self.send_available += amt;
                return Poll::Ready(Ok(self.send_available as usize));
            }
            Poll::Ready(None) => return Poll::Ready(Ok(0)),
            Poll::Pending => {}
        }
        Poll::Pending
    }
}

impl VirtualSocket for RemoteSocket {
    fn set_ttl(&mut self, ttl: u32) -> Result<()> {
        self.io_socket_fire_and_forget(RequestType::SetTtl(ttl))
    }

    fn ttl(&self) -> Result<u32> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetTtl)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Ttl(ttl) => Ok(ttl),
            res => {
                tracing::debug!("invalid response to get TTL request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    fn addr_local(&self) -> Result<SocketAddr> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetAddrLocal)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::SocketAddr(addr) => Ok(addr),
            res => {
                tracing::debug!("invalid response to address local request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    fn status(&self) -> Result<crate::SocketStatus> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetStatus)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Status(status) => Ok(status),
            res => {
                tracing::debug!("invalid response to status request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    fn set_handler(
        &mut self,
        handler: Box<dyn virtual_mio::InterestHandler + Send + Sync>,
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
        // We may already have accepted a connection in the `poll_read_ready` method
        self.touch_begin_accept()?;
        let accepted = if let Some(child) = self.buffer_accept.pop_front() {
            child
        } else {
            self.rx_accept.try_recv().map_err(|err| match err {
                TryRecvError::Empty => NetworkError::WouldBlock,
                TryRecvError::Disconnected => NetworkError::ConnectionAborted,
            })?
        };

        // This placed here will mean there is always an accept request pending at the
        // server as the constructor invokes this method and we invoke it here after
        // receiving a child connection.
        let mut rx_recv = None;
        if let Some((rx_socket, existing_rx_recv)) = self.pending_accept.take() {
            if accepted.socket == rx_socket {
                rx_recv.replace(existing_rx_recv);
            }
        }
        let rx_recv = match rx_recv {
            Some(rx_recv) => rx_recv,
            None => {
                let (tx, rx_recv) = tokio::sync::mpsc::channel(100);
                self.common
                    .recv_tx
                    .lock()
                    .unwrap()
                    .insert(accepted.socket, tx);
                rx_recv
            }
        };
        self.touch_begin_accept().ok();

        let (tx, rx_recv_with_addr) = tokio::sync::mpsc::channel(100);
        self.common
            .recv_with_addr_tx
            .lock()
            .unwrap()
            .insert(accepted.socket, tx);

        let (tx, rx_accept) = tokio::sync::mpsc::channel(100);
        self.common
            .accept_tx
            .lock()
            .unwrap()
            .insert(accepted.socket, tx);

        let (tx, rx_sent) = tokio::sync::mpsc::channel(100);
        self.common
            .sent_tx
            .lock()
            .unwrap()
            .insert(accepted.socket, tx);

        let socket = RemoteSocket {
            socket_id: accepted.socket,
            common: self.common.clone(),
            rx_buffer: BytesMut::new(),
            rx_recv,
            rx_recv_with_addr,
            rx_accept,
            rx_sent,
            pending_accept: None,
            tx_waker: TxWaker::new(&self.common).as_waker(),
            buffer_accept: Default::default(),
            buffer_recv_with_addr: Default::default(),
            send_available: 0,
        };
        Ok((Box::new(socket), accepted.addr))
    }

    fn set_handler(
        &mut self,
        handler: Box<dyn virtual_mio::InterestHandler + Send + Sync>,
    ) -> Result<()> {
        VirtualSocket::set_handler(self, handler)
    }

    fn addr_local(&self) -> Result<SocketAddr> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetAddrLocal)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::SocketAddr(addr) => Ok(addr),
            res => {
                tracing::debug!("invalid response to addr local request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    fn set_ttl(&mut self, ttl: u8) -> Result<()> {
        self.io_socket_fire_and_forget(RequestType::SetTtl(ttl as u32))
    }

    fn ttl(&self) -> Result<u8> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetTtl)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Ttl(val) => Ok(val.try_into().map_err(|_| NetworkError::InvalidData)?),
            res => {
                tracing::debug!("invalid response to get TTL request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }
}

impl VirtualRawSocket for RemoteSocket {
    fn try_send(&mut self, data: &[u8]) -> Result<usize> {
        let mut cx = Context::from_waker(&self.tx_waker);
        match self.common.tx.poll_send(
            &mut cx,
            MessageRequest::Send {
                socket: self.socket_id,
                data: data.to_vec(),
                req_id: None,
            },
        ) {
            Poll::Ready(Ok(())) => Ok(data.len()),
            Poll::Ready(Err(NetworkError::WouldBlock)) | Poll::Pending => {
                self.send_available = 0;
                Err(NetworkError::WouldBlock)
            }
            Poll::Ready(Err(err)) => Err(err),
        }
    }

    fn try_flush(&mut self) -> Result<()> {
        let mut cx = Context::from_waker(&self.tx_waker);
        match self.common.tx.poll_send(
            &mut cx,
            MessageRequest::Socket {
                socket: self.socket_id,
                req: RequestType::Flush,
                req_id: None,
            },
        ) {
            Poll::Ready(Ok(())) => Ok(()),
            Poll::Ready(Err(NetworkError::WouldBlock)) | Poll::Pending => {
                self.send_available = 0;
                Err(NetworkError::WouldBlock)
            }
            Poll::Ready(Err(err)) => Err(err),
        }
    }

    fn try_recv(&mut self, buf: &mut [std::mem::MaybeUninit<u8>]) -> Result<usize> {
        loop {
            if !self.rx_buffer.is_empty() {
                let amt = self.rx_buffer.len().min(buf.len());
                let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
                buf[..amt].copy_from_slice(&self.rx_buffer[..amt]);
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
        self.io_socket_fire_and_forget(RequestType::SetPromiscuous(promiscuous))
    }

    fn promiscuous(&self) -> Result<bool> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetPromiscuous)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Flag(val) => Ok(val),
            res => {
                tracing::debug!("invalid response to get promiscuous request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }
}

impl VirtualConnectionlessSocket for RemoteSocket {
    fn try_send_to(&mut self, data: &[u8], addr: SocketAddr) -> Result<usize> {
        let req_id = self.common.request_seed.fetch_add(1, Ordering::SeqCst);
        let mut cx = Context::from_waker(&self.tx_waker);
        match self.common.tx.poll_send(
            &mut cx,
            MessageRequest::SendTo {
                socket: self.socket_id,
                data: data.to_vec(),
                addr,
                req_id: Some(req_id),
            },
        ) {
            Poll::Ready(Ok(())) => Ok(data.len()),
            Poll::Ready(Err(NetworkError::WouldBlock)) | Poll::Pending => {
                self.send_available = 0;
                Err(NetworkError::WouldBlock)
            }
            Poll::Ready(Err(err)) => Err(err),
        }
    }

    fn try_recv_from(
        &mut self,
        buf: &mut [std::mem::MaybeUninit<u8>],
    ) -> Result<(usize, SocketAddr)> {
        match self.rx_recv_with_addr.try_recv() {
            Ok(received) => {
                let amt = buf.len().min(received.data.len());
                let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
                buf[..amt].copy_from_slice(&received.data[..amt]);
                Ok((amt, received.addr))
            }
            Err(TryRecvError::Disconnected) => Err(NetworkError::ConnectionAborted),
            Err(TryRecvError::Empty) => Err(NetworkError::WouldBlock),
        }
    }
}

impl VirtualUdpSocket for RemoteSocket {
    fn set_broadcast(&mut self, broadcast: bool) -> Result<()> {
        self.io_socket_fire_and_forget(RequestType::SetBroadcast(broadcast))
    }

    fn broadcast(&self) -> Result<bool> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetBroadcast)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Flag(val) => Ok(val),
            res => {
                tracing::debug!("invalid response to get broadcast request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    fn set_multicast_loop_v4(&mut self, val: bool) -> Result<()> {
        self.io_socket_fire_and_forget(RequestType::SetMulticastLoopV4(val))
    }

    fn multicast_loop_v4(&self) -> Result<bool> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetMulticastLoopV4)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Flag(val) => Ok(val),
            res => {
                tracing::debug!("invalid response to get multicast loop v4 request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    fn set_multicast_loop_v6(&mut self, val: bool) -> Result<()> {
        self.io_socket_fire_and_forget(RequestType::SetMulticastLoopV6(val))
    }

    fn multicast_loop_v6(&self) -> Result<bool> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetMulticastLoopV6)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Flag(val) => Ok(val),
            res => {
                tracing::debug!("invalid response to get multicast loop v6 request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    fn set_multicast_ttl_v4(&mut self, ttl: u32) -> Result<()> {
        self.io_socket_fire_and_forget(RequestType::SetMulticastTtlV4(ttl))
    }

    fn multicast_ttl_v4(&self) -> Result<u32> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetMulticastTtlV4)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Ttl(ttl) => Ok(ttl),
            res => {
                tracing::debug!("invalid response to get multicast TTL v4 request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    fn join_multicast_v4(
        &mut self,
        multiaddr: std::net::Ipv4Addr,
        iface: std::net::Ipv4Addr,
    ) -> Result<()> {
        self.io_socket_fire_and_forget(RequestType::JoinMulticastV4 { multiaddr, iface })
    }

    fn leave_multicast_v4(
        &mut self,
        multiaddr: std::net::Ipv4Addr,
        iface: std::net::Ipv4Addr,
    ) -> Result<()> {
        self.io_socket_fire_and_forget(RequestType::LeaveMulticastV4 { multiaddr, iface })
    }

    fn join_multicast_v6(&mut self, multiaddr: std::net::Ipv6Addr, iface: u32) -> Result<()> {
        self.io_socket_fire_and_forget(RequestType::JoinMulticastV6 { multiaddr, iface })
    }

    fn leave_multicast_v6(&mut self, multiaddr: std::net::Ipv6Addr, iface: u32) -> Result<()> {
        self.io_socket_fire_and_forget(RequestType::LeaveMulticastV6 { multiaddr, iface })
    }

    fn addr_peer(&self) -> Result<Option<SocketAddr>> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetAddrPeer)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(None),
            ResponseType::SocketAddr(addr) => Ok(Some(addr)),
            res => {
                tracing::debug!("invalid response to addr peer request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }
}

impl VirtualIcmpSocket for RemoteSocket {}

impl VirtualConnectedSocket for RemoteSocket {
    fn set_linger(&mut self, linger: Option<Duration>) -> Result<()> {
        self.io_socket_fire_and_forget(RequestType::SetLinger(linger))
    }

    fn linger(&self) -> Result<Option<Duration>> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetLinger)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(None),
            ResponseType::Duration(val) => Ok(Some(val)),
            res => {
                tracing::debug!("invalid response to get linger request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    fn try_send(&mut self, data: &[u8]) -> Result<usize> {
        let req_id = self.common.request_seed.fetch_add(1, Ordering::SeqCst);
        let mut cx = Context::from_waker(&self.tx_waker);
        match self.common.tx.poll_send(
            &mut cx,
            MessageRequest::Send {
                socket: self.socket_id,
                data: data.to_vec(),
                req_id: Some(req_id),
            },
        ) {
            Poll::Ready(Ok(())) => Ok(data.len()),
            Poll::Ready(Err(err)) => Err(err),
            Poll::Pending => Err(NetworkError::WouldBlock),
        }
    }

    fn try_flush(&mut self) -> Result<()> {
        let mut cx = Context::from_waker(&self.tx_waker);
        match self.common.tx.poll_send(
            &mut cx,
            MessageRequest::Socket {
                socket: self.socket_id,
                req: RequestType::Flush,
                req_id: None,
            },
        ) {
            Poll::Ready(Ok(())) => Ok(()),
            Poll::Ready(Err(err)) => Err(err),
            Poll::Pending => Err(NetworkError::WouldBlock),
        }
    }

    fn close(&mut self) -> Result<()> {
        self.io_socket_fire_and_forget(RequestType::Close)
    }

    fn try_recv(&mut self, buf: &mut [std::mem::MaybeUninit<u8>]) -> Result<usize> {
        loop {
            if !self.rx_buffer.is_empty() {
                let amt = self.rx_buffer.len().min(buf.len());
                let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
                buf[..amt].copy_from_slice(&self.rx_buffer[..amt]);
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
        self.io_socket_fire_and_forget(RequestType::SetRecvBufSize(size as u64))
    }

    fn recv_buf_size(&self) -> Result<usize> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetRecvBufSize)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Amount(amt) => Ok(amt.try_into().map_err(|_| NetworkError::IOError)?),
            res => {
                tracing::debug!("invalid response to get recv buf size request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    fn set_send_buf_size(&mut self, size: usize) -> Result<()> {
        self.io_socket_fire_and_forget(RequestType::SetSendBufSize(size as u64))
    }

    fn send_buf_size(&self) -> Result<usize> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetSendBufSize)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Amount(val) => Ok(val.try_into().map_err(|_| NetworkError::IOError)?),
            res => {
                tracing::debug!("invalid response to get send buf size request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    fn set_nodelay(&mut self, reuse: bool) -> Result<()> {
        self.io_socket_fire_and_forget(RequestType::SetNoDelay(reuse))
    }

    fn nodelay(&self) -> Result<bool> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetNoDelay)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Flag(val) => Ok(val),
            res => {
                tracing::debug!("invalid response to get nodelay request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    fn set_keepalive(&mut self, keep_alive: bool) -> Result<()> {
        self.io_socket_fire_and_forget(RequestType::SetKeepAlive(keep_alive))
    }

    fn keepalive(&self) -> Result<bool> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetKeepAlive)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Flag(val) => Ok(val),
            res => {
                tracing::debug!("invalid response to get nodelay request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    fn set_dontroute(&mut self, dont_route: bool) -> Result<()> {
        self.io_socket_fire_and_forget(RequestType::SetDontRoute(dont_route))
    }

    fn dontroute(&self) -> Result<bool> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetDontRoute)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Flag(val) => Ok(val),
            res => {
                tracing::debug!("invalid response to get nodelay request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    fn addr_peer(&self) -> Result<SocketAddr> {
        match InlineWaker::block_on(self.io_socket(RequestType::GetAddrPeer)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::SocketAddr(addr) => Ok(addr),
            res => {
                tracing::debug!("invalid response to addr peer request - {res:?}");
                Err(NetworkError::IOError)
            }
        }
    }

    fn shutdown(&mut self, how: std::net::Shutdown) -> Result<()> {
        let shutdown = match how {
            std::net::Shutdown::Read => meta::Shutdown::Read,
            std::net::Shutdown::Write => meta::Shutdown::Write,
            std::net::Shutdown::Both => meta::Shutdown::Both,
        };
        self.io_socket_fire_and_forget(RequestType::Shutdown(shutdown))
    }

    fn is_closed(&self) -> bool {
        match InlineWaker::block_on(self.io_socket(RequestType::IsClosed)) {
            ResponseType::Flag(val) => val,
            _ => false,
        }
    }
}
