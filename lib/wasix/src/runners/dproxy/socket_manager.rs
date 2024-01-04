use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use virtual_net::{tcp_pair::TcpSocketHalf, LoopbackNetworking};

#[derive(Debug)]
pub struct SocketManager {
    loopback_networking: LoopbackNetworking,
    proxy_connect_init_timeout: Duration,
    proxy_connect_nominal_timeout: Duration,
    is_running: AtomicBool,
}

impl SocketManager {
    pub fn new(
        loopback_networking: LoopbackNetworking,
        proxy_connect_init_timeout: Duration,
        proxy_connect_nominal_timeout: Duration,
    ) -> Self {
        Self {
            loopback_networking,
            proxy_connect_init_timeout,
            proxy_connect_nominal_timeout,
            is_running: AtomicBool::new(false),
        }
    }

    pub async fn acquire_http_socket(&self) -> anyhow::Result<TcpSocketHalf> {
        let connect_timeout = if self.is_running.load(Ordering::SeqCst) == true {
            self.proxy_connect_nominal_timeout
        } else {
            self.proxy_connect_init_timeout
        };

        let ret = tokio::time::timeout(connect_timeout, self.open_proxy_http_socket()).await??;
        self.is_running.store(true, Ordering::Relaxed);
        Ok(ret)
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
}
