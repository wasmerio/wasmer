use std::{
    future::poll_fn,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Duration,
};

use derivative::Derivative;
use tokio::sync::broadcast;
use virtual_net::{tcp_pair::TcpSocketHalf, LoopbackNetworking};

pub type PollListeningFn =
    Arc<dyn Fn(&mut Context<'_>) -> Poll<SocketAddr> + Send + Sync + 'static>;

#[derive(Derivative)]
#[derivative(Debug)]
pub struct SocketManager {
    #[derivative(Debug = "ignore")]
    poll_listening: PollListeningFn,
    loopback_networking: LoopbackNetworking,
    proxy_connect_init_timeout: Duration,
    proxy_connect_nominal_timeout: Duration,
    is_running: AtomicBool,
    is_terminated: AtomicBool,
    terminate_all: broadcast::Sender<()>,
}

impl SocketManager {
    pub fn new(
        poll_listening: PollListeningFn,
        loopback_networking: LoopbackNetworking,
        proxy_connect_init_timeout: Duration,
        proxy_connect_nominal_timeout: Duration,
    ) -> Self {
        Self {
            poll_listening,
            loopback_networking,
            proxy_connect_init_timeout,
            proxy_connect_nominal_timeout,
            is_running: AtomicBool::new(false),
            is_terminated: AtomicBool::new(false),
            terminate_all: broadcast::channel(1).0,
        }
    }

    pub fn shutdown(&self) {
        self.is_terminated.store(true, Ordering::SeqCst);
        self.terminate_all.send(()).ok();
    }

    pub fn terminate_rx(&self) -> broadcast::Receiver<()> {
        self.terminate_all.subscribe()
    }

    pub async fn acquire_http_socket(&self) -> anyhow::Result<TcpSocketHalf> {
        let mut rx_terminate = self.terminate_all.subscribe();

        if self.is_terminated.load(Ordering::SeqCst) {
            return Err(anyhow::anyhow!(
                "failed to open HTTP socket as the instance has terminated"
            ));
        }
        let connect_timeout = if self.is_running.load(Ordering::SeqCst) {
            self.proxy_connect_nominal_timeout
        } else {
            self.proxy_connect_init_timeout
        };

        let ret = tokio::select! {
            socket = tokio::time::timeout(connect_timeout, self.open_proxy_http_socket()) => socket??,
            _ = rx_terminate.recv() => {
                return Err(anyhow::anyhow!(
                    "failed to open HTTP socket as the instance has terminated"
                ));
            }
        };
        self.is_running.store(true, Ordering::Relaxed);
        Ok(ret)
    }

    pub async fn open_proxy_http_socket(&self) -> anyhow::Result<TcpSocketHalf> {
        // We need to find the destination address
        let poll_listening = self.poll_listening.clone();
        let port = poll_fn(|cx| poll_listening(cx)).await.port();
        let dst = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);

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
}
