use crate::meta::{FrameSerializationFormat, ResponseType};
use crate::rx_tx::{RemoteRx, RemoteTx, RemoteTxWakers};
use crate::{
    meta::{MessageRequest, MessageResponse, RequestType, SocketId},
    VirtualNetworking, VirtualRawSocket, VirtualTcpListener, VirtualTcpSocket, VirtualUdpSocket,
};
use crate::{IpCidr, IpRoute, NetworkError, StreamSecurity, VirtualIcmpSocket};
use derivative::Derivative;
use futures_util::stream::FuturesOrdered;
use futures_util::{future::BoxFuture, StreamExt};
use futures_util::{Sink, Stream};
use std::collections::HashSet;
use std::mem::MaybeUninit;
use std::net::IpAddr;
use std::task::Waker;
use std::time::Duration;
use std::{
    collections::HashMap,
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use tokio::sync::OwnedMutexGuard;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc,
};
use tokio_serde::formats::SymmetricalBincode;
#[cfg(feature = "cbor")]
use tokio_serde::formats::SymmetricalCbor;
#[cfg(feature = "json")]
use tokio_serde::formats::SymmetricalJson;
#[cfg(feature = "messagepack")]
use tokio_serde::formats::SymmetricalMessagePack;
use tokio_serde::SymmetricallyFramed;
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};
use wasmer_virtual_io::InterestHandler;

type BackgroundTask = Option<BoxFuture<'static, ()>>;

#[derive(Debug, Clone)]
pub struct RemoteNetworkingServer {
    #[allow(dead_code)]
    common: Arc<RemoteAdapterCommon>,
    inner: Arc<tokio::sync::Mutex<Box<dyn VirtualNetworking + Send + Sync + 'static>>>,
}

impl RemoteNetworkingServer {
    /// Creates a new interface on the remote location using
    /// a unique interface ID and a pair of channels
    pub fn new_from_mpsc(
        tx: mpsc::Sender<MessageResponse>,
        rx: mpsc::Receiver<MessageRequest>,
        inner: Box<dyn VirtualNetworking + Send + Sync + 'static>,
    ) -> (Self, RemoteNetworkingServerDriver) {
        let (tx_work, rx_work) = mpsc::unbounded_channel();
        let tx_wakers = RemoteTxWakers::default();

        let common = RemoteAdapterCommon {
            tx: RemoteTx::Mpsc {
                tx,
                work: tx_work,
                wakers: tx_wakers.clone(),
            },
            rx: Mutex::new(RemoteRx::Mpsc {
                rx,
                wakers: tx_wakers,
            }),
            sockets: Default::default(),
            socket_accept: Default::default(),
            handler: Default::default(),
            stall_rx: Default::default(),
        };
        let common = Arc::new(common);

        let inner = Arc::new(tokio::sync::Mutex::new(inner));
        let driver = RemoteNetworkingServerDriver {
            more_work: rx_work,
            tasks: Default::default(),
            common: common.clone(),
            inner: inner.clone(),
        };
        let networking = Self { common, inner };

        (networking, driver)
    }

    /// Creates a new interface on the remote location using
    /// a unique interface ID and a pair of channels
    pub fn new_from_stream<TX, RX>(
        tx: TX,
        rx: RX,
        format: FrameSerializationFormat,
        inner: Box<dyn VirtualNetworking + Send + Sync + 'static>,
    ) -> (Self, RemoteNetworkingServerDriver)
    where
        TX: AsyncWrite + Send + 'static,
        RX: AsyncRead + Send + 'static,
    {
        let tx = FramedWrite::new(tx, LengthDelimitedCodec::new());
        let tx: Pin<Box<dyn Sink<MessageResponse, Error = std::io::Error> + Send + 'static>> =
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
        let rx: Pin<Box<dyn Stream<Item = std::io::Result<MessageRequest>> + Send + 'static>> =
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

        let handler = RemoteAdapterHandler::default();
        let common = RemoteAdapterCommon {
            tx: RemoteTx::Stream {
                tx: Arc::new(tokio::sync::Mutex::new(tx)),
                work: tx_work,
                wakers: tx_wakers.clone(),
            },
            rx: Mutex::new(RemoteRx::Stream { rx }),
            sockets: Default::default(),
            socket_accept: Default::default(),
            handler,
            stall_rx: Default::default(),
        };
        let common = Arc::new(common);

        let inner = Arc::new(tokio::sync::Mutex::new(inner));
        let driver = RemoteNetworkingServerDriver {
            more_work: rx_work,
            tasks: Default::default(),
            common: common.clone(),
            inner: inner.clone(),
        };
        let networking = Self { common, inner };

        (networking, driver)
    }
}

#[async_trait::async_trait]
impl VirtualNetworking for RemoteNetworkingServer {
    async fn bridge(
        &self,
        network: &str,
        access_token: &str,
        security: StreamSecurity,
    ) -> Result<(), NetworkError> {
        let inner = self.inner.lock().await;
        inner.bridge(network, access_token, security).await
    }

    async fn unbridge(&self) -> Result<(), NetworkError> {
        let inner = self.inner.lock().await;
        inner.unbridge().await
    }

    async fn dhcp_acquire(&self) -> Result<Vec<IpAddr>, NetworkError> {
        let inner = self.inner.lock().await;
        inner.dhcp_acquire().await
    }

    fn ip_add(&self, ip: IpAddr, prefix: u8) -> Result<(), NetworkError> {
        let inner = self.inner.blocking_lock();
        inner.ip_add(ip, prefix)
    }

    fn ip_remove(&self, ip: IpAddr) -> Result<(), NetworkError> {
        let inner = self.inner.blocking_lock();
        inner.ip_remove(ip)
    }

    fn ip_clear(&self) -> Result<(), NetworkError> {
        let inner = self.inner.blocking_lock();
        inner.ip_clear()
    }

    fn ip_list(&self) -> Result<Vec<IpCidr>, NetworkError> {
        let inner = self.inner.blocking_lock();
        inner.ip_list()
    }

    fn mac(&self) -> Result<[u8; 6], NetworkError> {
        let inner = self.inner.blocking_lock();
        inner.mac()
    }

    fn gateway_set(&self, ip: IpAddr) -> Result<(), NetworkError> {
        let inner = self.inner.blocking_lock();
        inner.gateway_set(ip)
    }

    fn route_add(
        &self,
        cidr: IpCidr,
        via_router: IpAddr,
        preferred_until: Option<Duration>,
        expires_at: Option<Duration>,
    ) -> Result<(), NetworkError> {
        let inner = self.inner.blocking_lock();
        inner.route_add(cidr, via_router, preferred_until, expires_at)
    }

    fn route_remove(&self, cidr: IpAddr) -> Result<(), NetworkError> {
        let inner = self.inner.blocking_lock();
        inner.route_remove(cidr)
    }

    fn route_clear(&self) -> Result<(), NetworkError> {
        let inner = self.inner.blocking_lock();
        inner.route_clear()
    }

    fn route_list(&self) -> Result<Vec<IpRoute>, NetworkError> {
        let inner = self.inner.blocking_lock();
        inner.route_list()
    }

    async fn bind_raw(&self) -> Result<Box<dyn VirtualRawSocket + Sync>, NetworkError> {
        let inner = self.inner.lock().await;
        inner.bind_raw().await
    }

    async fn listen_tcp(
        &self,
        addr: SocketAddr,
        only_v6: bool,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> Result<Box<dyn VirtualTcpListener + Sync>, NetworkError> {
        let inner = self.inner.lock().await;
        inner
            .listen_tcp(addr, only_v6, reuse_port, reuse_addr)
            .await
    }

    async fn bind_udp(
        &self,
        addr: SocketAddr,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> Result<Box<dyn VirtualUdpSocket + Sync>, NetworkError> {
        let inner = self.inner.lock().await;
        inner.bind_udp(addr, reuse_port, reuse_addr).await
    }

    async fn bind_icmp(
        &self,
        addr: IpAddr,
    ) -> Result<Box<dyn VirtualIcmpSocket + Sync>, NetworkError> {
        let inner = self.inner.lock().await;
        inner.bind_icmp(addr).await
    }

    async fn connect_tcp(
        &self,
        addr: SocketAddr,
        peer: SocketAddr,
    ) -> Result<Box<dyn VirtualTcpSocket + Sync>, NetworkError> {
        let inner = self.inner.lock().await;
        inner.connect_tcp(addr, peer).await
    }

    async fn resolve(
        &self,
        host: &str,
        port: Option<u16>,
        dns_server: Option<IpAddr>,
    ) -> Result<Vec<IpAddr>, NetworkError> {
        let inner = self.inner.lock().await;
        inner.resolve(host, port, dns_server).await
    }
}

pin_project_lite::pin_project! {
    pub struct RemoteNetworkingServerDriver {
        common: Arc<RemoteAdapterCommon>,
        more_work: mpsc::UnboundedReceiver<BoxFuture<'static, ()>>,
        #[pin]
        tasks: FuturesOrdered<BoxFuture<'static, ()>>,
        inner: Arc<tokio::sync::Mutex<Box<dyn VirtualNetworking + Send + Sync + 'static>>>,
    }
}

impl Future for RemoteNetworkingServerDriver {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // We register the waker into the interest of the sockets so
        // that it is woken when something is ready to read or write
        let readable = {
            let mut guard = self.common.handler.state.lock().unwrap();
            if guard.driver_wakers.iter().any(|w| w.will_wake(cx.waker())) == false {
                guard.driver_wakers.push(cx.waker().clone());
            }
            guard.readable.drain().collect()
        };
        let readable: Vec<_> = readable;

        {
            // When a socket is marked as readable then we should drain all the data
            // from it and start sending it to the client
            let common = self.common.clone();
            let mut guard = common.sockets.lock().unwrap();
            for socket_id in readable {
                if let Some(task) = guard
                    .get_mut(&socket_id)
                    .map(|s| s.drain_reads_and_accepts(&common, socket_id))
                    .unwrap_or(None)
                {
                    self.tasks.push_back(task);
                }
            }
        }

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
            // which creates back pressure on the client so that they don't overload
            // the system
            match self.tasks.poll_next_unpin(cx) {
                Poll::Ready(Some(_)) => continue,
                Poll::Ready(None) => {
                    not_stalled_guard.take();
                }
                Poll::Pending if not_stalled_guard.is_none() => {
                    if let Ok(guard) = self.common.stall_rx.clone().try_lock_owned() {
                        not_stalled_guard.replace(guard);
                    } else {
                        return Poll::Pending;
                    }
                }
                Poll::Pending => {}
            };

            // We grab the next message sent by the client to us
            let msg = {
                let mut rx_guard = self.common.rx.lock().unwrap();
                rx_guard.poll(cx)
            };
            return match msg {
                Poll::Ready(Some(msg)) => {
                    if let Some(task) = self.process(msg) {
                        // With some messages we process there are background tasks that need to
                        // be further driver to completion by the driver
                        self.tasks.push_back(task)
                    };
                    continue;
                }
                Poll::Ready(None) => Poll::Ready(()),
                Poll::Pending => Poll::Pending,
            };
        }
    }
}

impl RemoteNetworkingServerDriver {
    fn process(&mut self, msg: MessageRequest) -> BackgroundTask {
        match msg {
            MessageRequest::Send {
                socket,
                data,
                req_id,
            } => self.process_send(socket, data, req_id),
            MessageRequest::SendTo {
                socket,
                data,
                addr,
                req_id,
            } => self.process_send_to(socket, data, addr, req_id),
            MessageRequest::Interface { req, req_id } => self.process_interface(req, req_id),
            MessageRequest::Socket {
                socket,
                req,
                req_id,
            } => self.process_socket(socket, req, req_id),
        }
    }

    fn process_send(
        &mut self,
        socket_id: SocketId,
        data: Vec<u8>,
        req_id: Option<u64>,
    ) -> BackgroundTask {
        let mut guard = self.common.sockets.lock().unwrap();
        guard
            .get_mut(&socket_id)
            .map(|s| {
                req_id
                    .map(|req_id| s.send(&self.common, socket_id, data, req_id))
                    .flatten()
            })
            .unwrap_or(None)
    }

    fn process_send_to(
        &mut self,
        socket_id: SocketId,
        data: Vec<u8>,
        addr: SocketAddr,
        req_id: Option<u64>,
    ) -> BackgroundTask {
        let mut guard = self.common.sockets.lock().unwrap();
        guard
            .get_mut(&socket_id)
            .map(|s| {
                req_id
                    .map(|req_id| s.send_to(&self.common, socket_id, data, addr, req_id))
                    .flatten()
            })
            .unwrap_or(None)
    }

    fn process_async<F>(future: F) -> BackgroundTask
    where
        F: Future<Output = BackgroundTask> + Send + 'static,
    {
        Some(Box::pin(async move {
            let background_task = future.await;
            if let Some(background_task) = background_task {
                background_task.await;
            }
        }))
    }

    fn process_async_inner<F, Fut, T>(
        &self,
        work: F,
        transmute: T,
        req_id: Option<u64>,
    ) -> BackgroundTask
    where
        F: FnOnce(OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>) -> Fut
            + Send
            + 'static,
        Fut: Future + Send + 'static,
        T: FnOnce(Fut::Output) -> ResponseType + Send + 'static,
    {
        let inner = self.inner.clone();
        let common = self.common.clone();
        Self::process_async(async move {
            let inner = inner.lock_owned().await;
            let future = work(inner);
            let ret = future.await;
            req_id
                .map(|req_id| {
                    common.send(MessageResponse::ResponseToRequest {
                        req_id,
                        res: transmute(ret),
                    })
                })
                .flatten()
        })
    }

    fn process_async_noop<F, Fut>(&self, work: F, req_id: Option<u64>) -> BackgroundTask
    where
        F: FnOnce(OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>) -> Fut
            + Send
            + 'static,
        Fut: Future<Output = Result<(), NetworkError>> + Send + 'static,
    {
        self.process_async_inner(
            work,
            move |ret| match ret {
                Ok(()) => ResponseType::None,
                Err(err) => ResponseType::Err(err),
            },
            req_id,
        )
    }

    fn process_async_new_socket<F, Fut>(
        &self,
        work: F,
        socket_id: SocketId,
        req_id: Option<u64>,
    ) -> BackgroundTask
    where
        F: FnOnce(OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>) -> Fut
            + Send
            + 'static,
        Fut: Future<Output = Result<RemoteAdapterSocket, NetworkError>> + Send + 'static,
    {
        let common = self.common.clone();
        self.process_async_inner(
            work,
            move |ret| match ret {
                Ok(mut socket) => {
                    let handler = Box::new(common.handler.clone().for_socket(socket_id));

                    let err = match &mut socket {
                        RemoteAdapterSocket::TcpListener { .. } => {
                            // we do not attach the handler immediately with new TPC listeners as we
                            // only want it to trigger when the BeginAccept message is received with
                            // a child ID we can actually use
                            Ok(())
                        }
                        RemoteAdapterSocket::TcpSocket(s) => s.set_handler(handler),
                        RemoteAdapterSocket::UdpSocket(s) => s.set_handler(handler),
                        RemoteAdapterSocket::IcmpSocket(s) => s.set_handler(handler),
                        RemoteAdapterSocket::RawSocket(s) => s.set_handler(handler),
                    };
                    if let Err(err) = err {
                        return ResponseType::Err(err);
                    }

                    let mut guard = common.sockets.lock().unwrap();
                    guard.insert(socket_id, socket);

                    ResponseType::Socket(socket_id)
                }
                Err(err) => ResponseType::Err(err),
            },
            req_id,
        )
    }

    fn process_inner<F, R, T>(
        &self,
        work: F,
        transmute: T,
        socket_id: SocketId,
        req_id: Option<u64>,
    ) -> BackgroundTask
    where
        F: FnOnce(&mut RemoteAdapterSocket) -> R + Send + 'static,
        T: FnOnce(R) -> ResponseType + Send + 'static,
    {
        let ret = {
            let mut guard = self.common.sockets.lock().unwrap();
            let socket = match guard.get_mut(&socket_id) {
                Some(s) => s,
                None => {
                    return req_id
                        .map(|req_id| {
                            self.common.send(MessageResponse::ResponseToRequest {
                                req_id,
                                res: ResponseType::Err(NetworkError::InvalidFd),
                            })
                        })
                        .flatten()
                }
            };
            work(socket)
        };
        req_id
            .map(|req_id| {
                self.common.send(MessageResponse::ResponseToRequest {
                    req_id,
                    res: transmute(ret),
                })
            })
            .flatten()
    }

    fn process_inner_noop<F>(
        &self,
        work: F,
        socket_id: SocketId,
        req_id: Option<u64>,
    ) -> BackgroundTask
    where
        F: FnOnce(&mut RemoteAdapterSocket) -> Result<(), NetworkError> + Send + 'static,
    {
        self.process_inner(
            work,
            move |ret| match ret {
                Ok(()) => ResponseType::None,
                Err(err) => ResponseType::Err(err),
            },
            socket_id,
            req_id,
        )
    }

    fn process_inner_begin_accept(
        &self,
        socket_id: SocketId,
        child_id: SocketId,
        req_id: Option<u64>,
    ) -> BackgroundTask {
        // We record the child socket so it can be used on the next accepted socket
        {
            let mut guard = self.common.socket_accept.lock().unwrap();
            guard.insert(socket_id, child_id);
        }

        // Now we attach the handler to the main listening socket
        let mut handler = Box::new(self.common.handler.clone().for_socket(socket_id));
        handler.interest(wasmer_virtual_io::InterestType::Readable);
        self.process_inner_noop(
            move |socket| match socket {
                RemoteAdapterSocket::TcpListener {
                    socket: s,
                    next_accept,
                    ..
                } => {
                    next_accept.replace(child_id);
                    s.set_handler(handler)
                }
                _ => {
                    // only the TCP listener needs its socket set as the other sockets
                    // set their handlers when the socket is created instead. we need to
                    // delay setting the handler so we have a child ID to use and return
                    // to the client when a socket is accepted, thus we can not accept them
                    // immediately
                    Err(NetworkError::Unsupported)
                }
            },
            socket_id,
            req_id,
        )
    }

    fn process_interface(&mut self, req: RequestType, req_id: Option<u64>) -> BackgroundTask {
        match req {
            RequestType::Bridge {
                network,
                access_token,
                security,
            } => self.process_async_noop(
                move |inner| async move { inner.bridge(&network, &access_token, security).await },
                req_id,
            ),
            RequestType::Unbridge => {
                self.process_async_noop(move |inner| async move { inner.unbridge().await }, req_id)
            }
            RequestType::DhcpAcquire => self.process_async_inner(
                move |inner| async move { inner.dhcp_acquire().await },
                |ret| match ret {
                    Ok(ips) => ResponseType::IpAddressList(ips),
                    Err(err) => ResponseType::Err(err),
                },
                req_id,
            ),
            RequestType::IpAdd { ip, prefix } => self.process_async_noop(
                move |inner: OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>| async move {
                    inner.ip_add(ip, prefix)
                },
                req_id,
            ),
            RequestType::IpRemove(ip) => self.process_async_noop(
                move |inner: OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>| async move {
                    inner.ip_remove(ip)
                },
                req_id,
            ),
            RequestType::IpClear => self.process_async_noop(
                move |inner: OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>| async move {
                    inner.ip_clear()
                },
                req_id,
            ),
            RequestType::GetIpList => self.process_async_inner(
                move |inner: OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>| async move {
                    inner.ip_list()
                },
                |ret| match ret {
                    Ok(cidr) => ResponseType::CidrList(cidr),
                    Err(err) => ResponseType::Err(err),
                },
                req_id,
            ),
            RequestType::GetMac => self.process_async_inner(
                move |inner: OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>| async move {
                    inner.mac()
                },
                |ret| match ret {
                    Ok(mac) => ResponseType::Mac(mac),
                    Err(err) => ResponseType::Err(err),
                },
                req_id,
            ),
            RequestType::GatewaySet(ip) => self.process_async_noop(
                move |inner: OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>| async move {
                    inner.gateway_set(ip)
                },
                req_id,
            ),
            RequestType::RouteAdd {
                cidr,
                via_router,
                preferred_until,
                expires_at,
            } => self.process_async_noop(
                move |inner: OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>| async move {
                    inner.route_add(cidr, via_router, preferred_until, expires_at)
                },
                req_id,
            ),
            RequestType::RouteRemove(ip) => self.process_async_noop(
                move |inner: OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>| async move {
                    inner.route_remove(ip)
                },
                req_id,
            ),
            RequestType::RouteClear => self.process_async_noop(
                move |inner: OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>| async move {
                    inner.route_clear()
                },
                req_id,
            ),
            RequestType::GetRouteList => self.process_async_inner(
                move |inner: OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>| async move {
                    inner.route_list()
                },
                |ret| match ret {
                    Ok(routes) => ResponseType::RouteList(routes),
                    Err(err) => ResponseType::Err(err),
                },
                req_id,
            ),
            RequestType::BindRaw(socket_id) => self.process_async_new_socket(
                move |inner: OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>| async move {
                    Ok(RemoteAdapterSocket::RawSocket(inner.bind_raw().await?))
                },
                socket_id,
                req_id,
            ),
            RequestType::ListenTcp {
                socket_id,
                addr,
                only_v6,
                reuse_port,
                reuse_addr,
            } => self.process_async_new_socket(
                move |inner: OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>| async move {
                    Ok(RemoteAdapterSocket::TcpListener {
                        socket: inner
                            .listen_tcp(addr, only_v6, reuse_port, reuse_addr)
                            .await?,
                        next_accept: None,
                    })
                },
                socket_id,
                req_id,
            ),
            RequestType::BindUdp {
                socket_id,
                addr,
                reuse_port,
                reuse_addr,
            } => self.process_async_new_socket(
                move |inner: OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>| async move {
                    Ok(RemoteAdapterSocket::UdpSocket(
                        inner.bind_udp(addr, reuse_port, reuse_addr).await?,
                    ))
                },
                socket_id,
                req_id,
            ),
            RequestType::BindIcmp { socket_id, addr } => self.process_async_new_socket(
                move |inner: OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>| async move {
                    Ok(RemoteAdapterSocket::IcmpSocket(
                        inner.bind_icmp(addr).await?,
                    ))
                },
                socket_id,
                req_id,
            ),
            RequestType::ConnectTcp {
                socket_id,
                addr,
                peer,
            } => self.process_async_new_socket(
                move |inner: OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>| async move {
                    Ok(RemoteAdapterSocket::TcpSocket(
                        inner.connect_tcp(addr, peer).await?,
                    ))
                },
                socket_id,
                req_id,
            ),
            RequestType::Resolve {
                host,
                port,
                dns_server,
            } => self.process_async_inner(
                move |inner: OwnedMutexGuard<Box<dyn VirtualNetworking + Send + Sync>>| async move {
                    inner.resolve(&host, port, dns_server).await
                },
                |ret| match ret {
                    Ok(ips) => ResponseType::IpAddressList(ips),
                    Err(err) => ResponseType::Err(err),
                },
                req_id,
            ),
            _ => req_id
                .map(|req_id| {
                    self.common.send(MessageResponse::ResponseToRequest {
                        req_id,
                        res: ResponseType::Err(NetworkError::Unsupported),
                    })
                })
                .flatten(),
        }
    }

    fn process_socket(
        &mut self,
        socket_id: SocketId,
        req: RequestType,
        req_id: Option<u64>,
    ) -> BackgroundTask {
        match req {
            RequestType::Flush => self.process_inner_noop(
                move |socket| match socket {
                    RemoteAdapterSocket::TcpSocket(s) => s.try_flush(),
                    RemoteAdapterSocket::RawSocket(s) => s.try_flush(),
                    _ => Err(NetworkError::Unsupported),
                },
                socket_id,
                req_id,
            ),
            RequestType::Close => self.process_inner_noop(
                move |socket| match socket {
                    RemoteAdapterSocket::TcpSocket(s) => s.close(),
                    _ => Err(NetworkError::Unsupported),
                },
                socket_id,
                req_id,
            ),
            RequestType::BeginAccept(child_id) => {
                self.process_inner_begin_accept(socket_id, child_id, req_id)
            }
            RequestType::GetAddrLocal => self.process_inner(
                move |socket| match socket {
                    RemoteAdapterSocket::TcpSocket(s) => s.addr_local(),
                    RemoteAdapterSocket::TcpListener { socket: s, .. } => s.addr_local(),
                    RemoteAdapterSocket::UdpSocket(s) => s.addr_local(),
                    RemoteAdapterSocket::IcmpSocket(s) => s.addr_local(),
                    RemoteAdapterSocket::RawSocket(s) => s.addr_local(),
                },
                |ret| match ret {
                    Ok(addr) => ResponseType::SocketAddr(addr),
                    Err(err) => ResponseType::Err(err),
                },
                socket_id,
                req_id,
            ),
            RequestType::GetAddrPeer => self.process_inner(
                move |socket| match socket {
                    RemoteAdapterSocket::TcpSocket(s) => s.addr_peer().map(Some),
                    RemoteAdapterSocket::TcpListener { .. } => Err(NetworkError::Unsupported),
                    RemoteAdapterSocket::UdpSocket(s) => s.addr_peer(),
                    RemoteAdapterSocket::IcmpSocket(_) => Err(NetworkError::Unsupported),
                    RemoteAdapterSocket::RawSocket(_) => Err(NetworkError::Unsupported),
                },
                |ret| match ret {
                    Ok(Some(addr)) => ResponseType::SocketAddr(addr),
                    Ok(None) => ResponseType::None,
                    Err(err) => ResponseType::Err(err),
                },
                socket_id,
                req_id,
            ),
            RequestType::SetTtl(ttl) => self.process_inner_noop(
                move |socket| match socket {
                    RemoteAdapterSocket::TcpSocket(s) => s.set_ttl(ttl),
                    RemoteAdapterSocket::TcpListener { socket: s, .. } => {
                        s.set_ttl(ttl.try_into().unwrap_or_default())
                    }
                    RemoteAdapterSocket::UdpSocket(s) => s.set_ttl(ttl),
                    RemoteAdapterSocket::IcmpSocket(s) => s.set_ttl(ttl),
                    RemoteAdapterSocket::RawSocket(s) => s.set_ttl(ttl),
                },
                socket_id,
                req_id,
            ),
            RequestType::GetTtl => self.process_inner(
                move |socket| match socket {
                    RemoteAdapterSocket::TcpSocket(s) => s.ttl(),
                    RemoteAdapterSocket::TcpListener { socket: s, .. } => s.ttl().map(|t| t as u32),
                    RemoteAdapterSocket::UdpSocket(s) => s.ttl(),
                    RemoteAdapterSocket::IcmpSocket(s) => s.ttl(),
                    RemoteAdapterSocket::RawSocket(s) => s.ttl(),
                },
                |ret| match ret {
                    Ok(ttl) => ResponseType::Ttl(ttl),
                    Err(err) => ResponseType::Err(err),
                },
                socket_id,
                req_id,
            ),
            RequestType::GetStatus => self.process_inner(
                move |socket| match socket {
                    RemoteAdapterSocket::TcpSocket(s) => s.status(),
                    RemoteAdapterSocket::TcpListener { .. } => Err(NetworkError::Unsupported),
                    RemoteAdapterSocket::UdpSocket(s) => s.status(),
                    RemoteAdapterSocket::IcmpSocket(s) => s.status(),
                    RemoteAdapterSocket::RawSocket(s) => s.status(),
                },
                |ret| match ret {
                    Ok(status) => ResponseType::Status(status),
                    Err(err) => ResponseType::Err(err),
                },
                socket_id,
                req_id,
            ),
            RequestType::SetLinger(linger) => self.process_inner_noop(
                move |socket| match socket {
                    RemoteAdapterSocket::TcpSocket(s) => s.set_linger(linger),
                    _ => Err(NetworkError::Unsupported),
                },
                socket_id,
                req_id,
            ),
            RequestType::GetLinger => self.process_inner(
                move |socket| match socket {
                    RemoteAdapterSocket::TcpSocket(s) => s.linger(),
                    _ => Err(NetworkError::Unsupported),
                },
                |ret| match ret {
                    Ok(Some(time)) => ResponseType::Duration(time),
                    Ok(None) => ResponseType::None,
                    Err(err) => ResponseType::Err(err),
                },
                socket_id,
                req_id,
            ),
            RequestType::SetPromiscuous(promiscuous) => self.process_inner_noop(
                move |socket| match socket {
                    RemoteAdapterSocket::RawSocket(s) => s.set_promiscuous(promiscuous),
                    _ => Err(NetworkError::Unsupported),
                },
                socket_id,
                req_id,
            ),
            RequestType::GetPromiscuous => self.process_inner(
                move |socket| match socket {
                    RemoteAdapterSocket::RawSocket(s) => s.promiscuous(),
                    _ => Err(NetworkError::Unsupported),
                },
                |ret| match ret {
                    Ok(flag) => ResponseType::Flag(flag),
                    Err(err) => ResponseType::Err(err),
                },
                socket_id,
                req_id,
            ),
            RequestType::SetRecvBufSize(size) => self.process_inner_noop(
                move |socket| match socket {
                    RemoteAdapterSocket::TcpSocket(s) => {
                        s.set_recv_buf_size(size.try_into().unwrap_or_default())
                    }
                    _ => Err(NetworkError::Unsupported),
                },
                socket_id,
                req_id,
            ),
            RequestType::GetRecvBufSize => self.process_inner(
                move |socket| match socket {
                    RemoteAdapterSocket::TcpSocket(s) => s.recv_buf_size(),
                    _ => Err(NetworkError::Unsupported),
                },
                |ret| match ret {
                    Ok(amt) => ResponseType::Amount(amt as u64),
                    Err(err) => ResponseType::Err(err),
                },
                socket_id,
                req_id,
            ),
            RequestType::SetSendBufSize(size) => self.process_inner_noop(
                move |socket| match socket {
                    RemoteAdapterSocket::TcpSocket(s) => {
                        s.set_send_buf_size(size.try_into().unwrap_or_default())
                    }
                    _ => Err(NetworkError::Unsupported),
                },
                socket_id,
                req_id,
            ),
            RequestType::GetSendBufSize => self.process_inner(
                move |socket| match socket {
                    RemoteAdapterSocket::TcpSocket(s) => s.send_buf_size(),
                    _ => Err(NetworkError::Unsupported),
                },
                |ret| match ret {
                    Ok(amt) => ResponseType::Amount(amt as u64),
                    Err(err) => ResponseType::Err(err),
                },
                socket_id,
                req_id,
            ),
            RequestType::SetNoDelay(reuse) => self.process_inner_noop(
                move |socket| match socket {
                    RemoteAdapterSocket::TcpSocket(s) => s.set_nodelay(reuse),
                    _ => Err(NetworkError::Unsupported),
                },
                socket_id,
                req_id,
            ),
            RequestType::GetNoDelay => self.process_inner(
                move |socket| match socket {
                    RemoteAdapterSocket::TcpSocket(s) => s.nodelay(),
                    _ => Err(NetworkError::Unsupported),
                },
                |ret| match ret {
                    Ok(flag) => ResponseType::Flag(flag),
                    Err(err) => ResponseType::Err(err),
                },
                socket_id,
                req_id,
            ),
            RequestType::Shutdown(shutdown) => self.process_inner_noop(
                move |socket| match socket {
                    RemoteAdapterSocket::TcpSocket(s) => s.shutdown(match shutdown {
                        crate::meta::Shutdown::Read => std::net::Shutdown::Read,
                        crate::meta::Shutdown::Write => std::net::Shutdown::Write,
                        crate::meta::Shutdown::Both => std::net::Shutdown::Both,
                    }),
                    _ => Err(NetworkError::Unsupported),
                },
                socket_id,
                req_id,
            ),
            RequestType::IsClosed => self.process_inner(
                move |socket| match socket {
                    RemoteAdapterSocket::TcpSocket(s) => Ok(s.is_closed()),
                    _ => Err(NetworkError::Unsupported),
                },
                |ret| match ret {
                    Ok(flag) => ResponseType::Flag(flag),
                    Err(err) => ResponseType::Err(err),
                },
                socket_id,
                req_id,
            ),
            RequestType::SetBroadcast(broadcast) => self.process_inner_noop(
                move |socket| match socket {
                    RemoteAdapterSocket::UdpSocket(s) => s.set_broadcast(broadcast),
                    _ => Err(NetworkError::Unsupported),
                },
                socket_id,
                req_id,
            ),
            RequestType::GetBroadcast => self.process_inner(
                move |socket| match socket {
                    RemoteAdapterSocket::UdpSocket(s) => s.broadcast(),
                    _ => Err(NetworkError::Unsupported),
                },
                |ret| match ret {
                    Ok(flag) => ResponseType::Flag(flag),
                    Err(err) => ResponseType::Err(err),
                },
                socket_id,
                req_id,
            ),
            RequestType::SetMulticastLoopV4(val) => self.process_inner_noop(
                move |socket| match socket {
                    RemoteAdapterSocket::UdpSocket(s) => s.set_multicast_loop_v4(val),
                    _ => Err(NetworkError::Unsupported),
                },
                socket_id,
                req_id,
            ),
            RequestType::GetMulticastLoopV4 => self.process_inner(
                move |socket| match socket {
                    RemoteAdapterSocket::UdpSocket(s) => s.multicast_loop_v4(),
                    _ => Err(NetworkError::Unsupported),
                },
                |ret| match ret {
                    Ok(flag) => ResponseType::Flag(flag),
                    Err(err) => ResponseType::Err(err),
                },
                socket_id,
                req_id,
            ),
            RequestType::SetMulticastLoopV6(val) => self.process_inner_noop(
                move |socket| match socket {
                    RemoteAdapterSocket::UdpSocket(s) => s.set_multicast_loop_v6(val),
                    _ => Err(NetworkError::Unsupported),
                },
                socket_id,
                req_id,
            ),
            RequestType::GetMulticastLoopV6 => self.process_inner(
                move |socket| match socket {
                    RemoteAdapterSocket::UdpSocket(s) => s.multicast_loop_v6(),
                    _ => Err(NetworkError::Unsupported),
                },
                |ret| match ret {
                    Ok(flag) => ResponseType::Flag(flag),
                    Err(err) => ResponseType::Err(err),
                },
                socket_id,
                req_id,
            ),
            RequestType::SetMulticastTtlV4(ttl) => self.process_inner_noop(
                move |socket| match socket {
                    RemoteAdapterSocket::UdpSocket(s) => s.set_multicast_ttl_v4(ttl),
                    _ => Err(NetworkError::Unsupported),
                },
                socket_id,
                req_id,
            ),
            RequestType::GetMulticastTtlV4 => self.process_inner(
                move |socket| match socket {
                    RemoteAdapterSocket::UdpSocket(s) => s.multicast_ttl_v4(),
                    _ => Err(NetworkError::Unsupported),
                },
                |ret| match ret {
                    Ok(ttl) => ResponseType::Ttl(ttl),
                    Err(err) => ResponseType::Err(err),
                },
                socket_id,
                req_id,
            ),
            RequestType::JoinMulticastV4 { multiaddr, iface } => self.process_inner_noop(
                move |socket| match socket {
                    RemoteAdapterSocket::UdpSocket(s) => s.join_multicast_v4(multiaddr, iface),
                    _ => Err(NetworkError::Unsupported),
                },
                socket_id,
                req_id,
            ),
            RequestType::LeaveMulticastV4 { multiaddr, iface } => self.process_inner_noop(
                move |socket| match socket {
                    RemoteAdapterSocket::UdpSocket(s) => s.leave_multicast_v4(multiaddr, iface),
                    _ => Err(NetworkError::Unsupported),
                },
                socket_id,
                req_id,
            ),
            RequestType::JoinMulticastV6 { multiaddr, iface } => self.process_inner_noop(
                move |socket| match socket {
                    RemoteAdapterSocket::UdpSocket(s) => s.join_multicast_v6(multiaddr, iface),
                    _ => Err(NetworkError::Unsupported),
                },
                socket_id,
                req_id,
            ),
            RequestType::LeaveMulticastV6 { multiaddr, iface } => self.process_inner_noop(
                move |socket| match socket {
                    RemoteAdapterSocket::UdpSocket(s) => s.leave_multicast_v6(multiaddr, iface),
                    _ => Err(NetworkError::Unsupported),
                },
                socket_id,
                req_id,
            ),
            _ => req_id
                .map(|req_id| {
                    self.common.send(MessageResponse::ResponseToRequest {
                        req_id,
                        res: ResponseType::Err(NetworkError::Unsupported),
                    })
                })
                .flatten(),
        }
    }
}

enum RemoteAdapterSocket {
    TcpListener {
        socket: Box<dyn VirtualTcpListener + Sync + 'static>,
        next_accept: Option<SocketId>,
    },
    TcpSocket(Box<dyn VirtualTcpSocket + Sync + 'static>),
    UdpSocket(Box<dyn VirtualUdpSocket + Sync + 'static>),
    RawSocket(Box<dyn VirtualRawSocket + Sync + 'static>),
    IcmpSocket(Box<dyn VirtualIcmpSocket + Sync + 'static>),
}

impl RemoteAdapterSocket {
    pub fn send(
        &mut self,
        common: &Arc<RemoteAdapterCommon>,
        socket_id: SocketId,
        data: Vec<u8>,
        req_id: u64,
    ) -> BackgroundTask {
        match self {
            Self::TcpSocket(this) => match this.try_send(&data) {
                Ok(amount) => common.send(MessageResponse::Sent {
                    socket_id,
                    req_id,
                    amount: amount as u64,
                }),
                Err(NetworkError::WouldBlock) => {
                    let common = common.clone();
                    Some(Box::pin(async move {
                        // We will stall the receiver so that back pressure is sent back to the
                        // sender and they don't overwhelm us with transmitting data.
                        let _stall_rx = common.stall_rx.clone().lock_owned().await;

                        // We use a poller here that uses the handler to wake itself up
                        struct Poller {
                            common: Arc<RemoteAdapterCommon>,
                            socket_id: SocketId,
                            data: Vec<u8>,
                            req_id: u64,
                        }
                        impl Future for Poller {
                            type Output = BackgroundTask;
                            fn poll(
                                self: Pin<&mut Self>,
                                cx: &mut Context<'_>,
                            ) -> Poll<Self::Output> {
                                // We make sure the waker is registered with the interest driver which will
                                // wake up this poller when there is writeability
                                let mut guard = self.common.handler.state.lock().unwrap();
                                if guard.driver_wakers.iter().any(|w| w.will_wake(cx.waker()))
                                    == false
                                {
                                    guard.driver_wakers.push(cx.waker().clone());
                                }
                                drop(guard);

                                let mut guard = self.common.sockets.lock().unwrap();
                                if let Some(socket) = guard.get_mut(&self.socket_id) {
                                    if let RemoteAdapterSocket::TcpSocket(socket) = socket {
                                        match socket.try_send(&self.data) {
                                            Ok(amount) => {
                                                return Poll::Ready(self.common.send(
                                                    MessageResponse::Sent {
                                                        socket_id: self.socket_id,
                                                        req_id: self.req_id,
                                                        amount: amount as u64,
                                                    },
                                                ))
                                            }
                                            Err(NetworkError::WouldBlock) => return Poll::Pending,
                                            Err(error) => {
                                                return Poll::Ready(self.common.send(
                                                    MessageResponse::SendError {
                                                        socket_id: self.socket_id,
                                                        req_id: self.req_id,
                                                        error,
                                                    },
                                                ))
                                            }
                                        }
                                    }
                                }
                                Poll::Ready(None)
                            }
                        }

                        // Run the poller until this message is sent, or the socket fails
                        let background_task = Poller {
                            common,
                            socket_id,
                            data,
                            req_id,
                        }
                        .await;

                        // There might be more work left to finish off the send operation
                        if let Some(background_task) = background_task {
                            background_task.await;
                        }
                    }))
                }
                Err(error) => common.send(MessageResponse::SendError {
                    socket_id,
                    req_id,
                    error,
                }),
            },
            Self::RawSocket(this) => {
                // when the RAW socket is overloaded we just silently drop the packet
                // rather than buffering it and retrying later - Ethernet packets are
                // not lossless. In reality most socket drivers under this remote socket
                // will always succeed on `try_send` with RawSockets as they are always
                // processed.
                this.try_send(&data).ok();
                None
            }
            _ => common.send(MessageResponse::SendError {
                socket_id,
                req_id,
                error: NetworkError::Unsupported,
            }),
        }
    }
    pub fn send_to(
        &mut self,
        common: &Arc<RemoteAdapterCommon>,
        socket_id: SocketId,
        data: Vec<u8>,
        addr: SocketAddr,
        req_id: u64,
    ) -> BackgroundTask {
        match self {
            Self::UdpSocket(this) => {
                // when the UDP socket is overloaded we just silently drop the packet
                // rather than buffering it and retrying later
                this.try_send_to(&data, addr).ok();
                None
            }

            Self::IcmpSocket(this) => {
                // when the ICMP socket is overloaded we just silently drop the packet
                // rather than buffering it and retrying later
                this.try_send_to(&data, addr).ok();
                None
            }
            _ => common.send(MessageResponse::SendError {
                socket_id,
                req_id,
                error: NetworkError::Unsupported,
            }),
        }
    }
    pub fn drain_reads_and_accepts(
        &mut self,
        common: &Arc<RemoteAdapterCommon>,
        socket_id: SocketId,
    ) -> BackgroundTask {
        // We loop reading the socket until all the pending reads are either
        // being processed in a background task or they are empty
        let mut ret: FuturesOrdered<BoxFuture<'static, ()>> = Default::default();
        loop {
            break match self {
                Self::TcpListener {
                    socket,
                    next_accept,
                } => {
                    if next_accept.is_some() {
                        match socket.try_accept() {
                            Ok((mut child_socket, addr)) => {
                                let child_id = next_accept.take().unwrap();

                                // We set the handler on the socket so that it can
                                let handler = Box::new(common.handler.clone().for_socket(child_id));
                                child_socket.set_handler(handler).ok();

                                // We will fix up the socket in the background then notify
                                // the client of the new socket
                                let common = common.clone();
                                ret.push_back(Box::pin(async move {
                                    // Next we record the socket so that it is active
                                    {
                                        let child_socket =
                                            RemoteAdapterSocket::TcpSocket(child_socket);
                                        let mut guard = common.sockets.lock().unwrap();
                                        guard.insert(child_id, child_socket);
                                    }

                                    // Lastly we tell the client about the new socket
                                    if let Some(task) = common.send(MessageResponse::FinishAccept {
                                        socket_id,
                                        child_id,
                                        addr,
                                    }) {
                                        task.await;
                                    }
                                }));
                            }
                            Err(NetworkError::WouldBlock) => {}
                            Err(err) => {
                                tracing::error!("failed to accept socket - {}", err);
                            }
                        }
                    }
                }
                Self::TcpSocket(this) => {
                    let mut chunk: [MaybeUninit<u8>; 10240] =
                        unsafe { MaybeUninit::uninit().assume_init() };
                    match this.try_recv(&mut chunk) {
                        Ok(0) => {}
                        Ok(amt) => {
                            let chunk_unsafe: &mut [MaybeUninit<u8>] = &mut chunk[..amt];
                            let chunk_unsafe: &mut [u8] =
                                unsafe { std::mem::transmute(chunk_unsafe) };
                            if let Some(task) = common.send(MessageResponse::Recv {
                                socket_id,
                                data: chunk_unsafe.to_vec(),
                            }) {
                                ret.push_back(task);
                            }
                            continue;
                        }
                        Err(_) => {}
                    }
                }
                Self::UdpSocket(this) => {
                    let mut chunk: [MaybeUninit<u8>; 10240] =
                        unsafe { MaybeUninit::uninit().assume_init() };
                    match this.try_recv_from(&mut chunk) {
                        Ok((0, _)) => {}
                        Ok((amt, addr)) => {
                            let chunk_unsafe: &mut [MaybeUninit<u8>] = &mut chunk[..amt];
                            let chunk_unsafe: &mut [u8] =
                                unsafe { std::mem::transmute(chunk_unsafe) };
                            if let Some(task) = common.send(MessageResponse::RecvWithAddr {
                                socket_id,
                                data: chunk_unsafe.to_vec(),
                                addr,
                            }) {
                                ret.push_back(task);
                            }
                        }
                        Err(_) => {}
                    }
                }
                Self::IcmpSocket(this) => {
                    let mut chunk: [MaybeUninit<u8>; 10240] =
                        unsafe { MaybeUninit::uninit().assume_init() };
                    match this.try_recv_from(&mut chunk) {
                        Ok((0, _)) => {}
                        Ok((amt, addr)) => {
                            let chunk_unsafe: &mut [MaybeUninit<u8>] = &mut chunk[..amt];
                            let chunk_unsafe: &mut [u8] =
                                unsafe { std::mem::transmute(chunk_unsafe) };
                            if let Some(task) = common.send(MessageResponse::RecvWithAddr {
                                socket_id,
                                data: chunk_unsafe.to_vec(),
                                addr,
                            }) {
                                ret.push_back(task);
                            }
                        }
                        Err(_) => {}
                    }
                }
                Self::RawSocket(this) => {
                    let mut chunk: [MaybeUninit<u8>; 10240] =
                        unsafe { MaybeUninit::uninit().assume_init() };
                    match this.try_recv(&mut chunk) {
                        Ok(0) => {}
                        Ok(amt) => {
                            let chunk_unsafe: &mut [MaybeUninit<u8>] = &mut chunk[..amt];
                            let chunk_unsafe: &mut [u8] =
                                unsafe { std::mem::transmute(chunk_unsafe) };
                            if let Some(task) = common.send(MessageResponse::Recv {
                                socket_id,
                                data: chunk_unsafe.to_vec(),
                            }) {
                                ret.push_back(task);
                            }
                        }
                        Err(_) => {}
                    }
                }
            };
        }

        if ret.is_empty() {
            // There is nothing to process so we are done
            None
        } else {
            Some(Box::pin(async move {
                // Processes all the background tasks until completion
                let mut stream = ret;
                loop {
                    let (next, s) = stream.into_future().await;
                    if next.is_none() {
                        break;
                    }
                    stream = s;
                }
            }))
        }
    }
}

#[derive(Debug, Default)]
struct RemoteAdapterHandlerState {
    readable: HashSet<SocketId>,
    driver_wakers: Vec<Waker>,
}

#[derive(Debug, Clone)]
struct RemoteAdapterHandler {
    socket_id: Option<SocketId>,
    state: Arc<Mutex<RemoteAdapterHandlerState>>,
}
impl Default for RemoteAdapterHandler {
    fn default() -> Self {
        Self {
            socket_id: None,
            state: Default::default(),
        }
    }
}
impl RemoteAdapterHandler {
    pub fn for_socket(self, id: SocketId) -> Self {
        Self {
            socket_id: Some(id),
            state: self.state,
        }
    }
}
impl InterestHandler for RemoteAdapterHandler {
    fn interest(&mut self, interest: wasmer_virtual_io::InterestType) {
        let mut guard = self.state.lock().unwrap();
        guard.driver_wakers.drain(..).for_each(|w| w.wake());
        let socket_id = match self.socket_id.clone() {
            Some(s) => s,
            None => return,
        };
        match interest {
            wasmer_virtual_io::InterestType::Readable => {
                guard.readable.insert(socket_id);
            }
            _ => {}
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
struct RemoteAdapterCommon {
    #[derivative(Debug = "ignore")]
    tx: RemoteTx<MessageResponse>,
    #[derivative(Debug = "ignore")]
    rx: Mutex<RemoteRx<MessageRequest>>,
    #[derivative(Debug = "ignore")]
    sockets: Mutex<HashMap<SocketId, RemoteAdapterSocket>>,
    socket_accept: Mutex<HashMap<SocketId, SocketId>>,
    handler: RemoteAdapterHandler,

    // The stall guard will prevent reads while its held and there are background tasks running
    // (the idea behind this is to create back pressure so that the task list infinitely grow)
    stall_rx: Arc<tokio::sync::Mutex<()>>,
}
impl RemoteAdapterCommon {
    fn send(self: &Arc<Self>, req: MessageResponse) -> BackgroundTask {
        let this = self.clone();
        Some(Box::pin(async move {
            if let Err(err) = this.tx.send(req).await {
                tracing::debug!("failed to send message - {}", err);
            }
        }))
    }
}
