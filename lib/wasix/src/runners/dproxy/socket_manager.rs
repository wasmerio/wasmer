use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use tokio::sync::mpsc::{
    self,
    error::{TryRecvError, TrySendError},
};
use virtual_net::{LoopbackNetworking, TcpSocketHalf};

#[derive(Debug)]
pub struct SocketManager {
    loopback_networking: LoopbackNetworking,
    idle_http_sockets: tokio::sync::Mutex<mpsc::Receiver<TcpSocketHalf>>,
    send_socket_idle: Arc<mpsc::Sender<TcpSocketHalf>>,
    proxy_connect_init_timeout: Duration,
    proxy_connect_nominal_timeout: Duration,
    maximum_idle_size: usize,
    is_running: AtomicBool,
}

impl SocketManager {
    pub fn new(
        loopback_networking: LoopbackNetworking,
        proxy_connect_init_timeout: Duration,
        proxy_connect_nominal_timeout: Duration,
        maximum_idle_size: usize,
    ) -> Self {
        let maximum_idle_size_channel = 1usize.max(maximum_idle_size);
        let (tx_idle, rx_idle) = mpsc::channel(maximum_idle_size_channel);
        Self {
            loopback_networking,
            idle_http_sockets: tokio::sync::Mutex::new(rx_idle),
            send_socket_idle: Arc::new(tx_idle),
            proxy_connect_init_timeout,
            proxy_connect_nominal_timeout,
            maximum_idle_size,
            is_running: AtomicBool::new(false),
        }
    }

    pub async fn acquire_http_socket(&self, reuse: bool) -> anyhow::Result<TcpSocketHalf> {
        loop {
            // We will only reuse the socket connection if its an idempotent as sockets that
            // have been closed by the server can not be retried otherwise
            let socket = if reuse {
                if let Ok(mut guard) = self.idle_http_sockets.try_lock() {
                    guard.try_recv().ok()
                } else {
                    None
                }
            } else {
                None
            };

            // Check the socket is active if it is not then
            // we need to open a new socket
            let ret = match socket {
                Some(s) if s.is_active() => {
                    tracing::trace!("reusing socket for proxy handler (R1)");
                    Ok(s)
                }
                Some(_) => {
                    tracing::trace!("socket in pool is no long active - trying again");
                    continue;
                }
                None => {
                    // Determine which timeout to use
                    let connect_timeout = if self.is_running.load(Ordering::SeqCst) == true {
                        self.proxy_connect_nominal_timeout
                    } else {
                        self.proxy_connect_init_timeout
                    };

                    // Use a guard as without this concurrent connects seem to timeout
                    // due to a race condition
                    tracing::trace!("opening new socket for proxy handler");
                    if let Some(socket) = self.try_reuse_proxy_http_socket().await? {
                        Ok(socket)
                    } else {
                        tokio::time::timeout(connect_timeout, self.open_proxy_http_socket()).await?
                    }
                }
            };

            match ret.as_ref() {
                Err(err) => {
                    tracing::debug!("connection attempt failed - {}", err);
                }
                Ok(_) => {
                    // If we have successfully acquired a HTTP connection then we are
                    // allowed to taint the instance on future failures
                    self.is_running.store(true, Ordering::Relaxed)
                }
            }

            return ret;
        }
    }

    pub async fn reuse_proxy_http_socket(&self) -> anyhow::Result<TcpSocketHalf> {
        let mut rx_socket = self.idle_http_sockets.lock().await;
        let socket = rx_socket.recv().await;
        if socket.is_some() {
            tracing::trace!("reusing socket for proxy handler (R2)");
        }
        socket.ok_or(anyhow::format_err!("proxy has shutdown"))
    }

    pub async fn try_reuse_proxy_http_socket(&self) -> anyhow::Result<Option<TcpSocketHalf>> {
        let mut rx_socket = self.idle_http_sockets.lock().await;
        let socket = match rx_socket.try_recv() {
            Ok(socket) => {
                tracing::trace!("reusing socket for proxy handler (R3)");
                Some(socket)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                return Err(anyhow::format_err!("proxy has shutdown"));
            }
        };
        Ok(socket)
    }

    pub async fn open_proxy_http_socket(&self) -> anyhow::Result<TcpSocketHalf> {
        let dst = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 80);

        // Open a connection directly to the loopback port
        // (or at least try to)
        self.loopback_networking
            .loopback_connect_to(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0), dst)
            .ok_or_else(|| {
                tracing::debug!(
                    "proxy connection attempt failed - could not connect to http server socket as the loopback socket is not open",
                );
                anyhow::anyhow!("failed to open HTTP socket as the loopback socket is not open")
            })
    }

    pub fn return_http_socket(&self, socket: TcpSocketHalf) {
        if self.maximum_idle_size > 0 {
            tracing::trace!("reusing active socket");
            match self.send_socket_idle.try_send(socket) {
                Err(TrySendError::Closed(socket)) => {
                    tracing::trace!("closing socket (reuse pipe is closed)");
                    socket.close().ok();
                }
                Err(TrySendError::Full(socket)) => {
                    tracing::trace!("closing socket (too many sockets)");
                    socket.close().ok();
                }
                Ok(_) => {}
            }
        } else {
            tracing::trace!("closing socket (reuse is disabled)");
            socket.close().ok();
        }
    }
}
