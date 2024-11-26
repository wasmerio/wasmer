use std::net::SocketAddr;
use std::task::{Context, Poll};

use crate::{Ipv4Addr, Ipv6Addr, NetworkError, VirtualIoSource, VirtualTcpListener};
use virtual_mio::ArcInterestHandler;

#[derive(Debug)]
pub struct CompositeTcpListener {
    ports: Vec<Box<dyn VirtualTcpListener + Sync>>,
}

impl CompositeTcpListener {
    pub fn new() -> Self {
        Self { ports: Vec::new() }
    }

    pub fn add_port(&mut self, port: Box<dyn VirtualTcpListener + Sync>) {
        self.ports.push(port);
    }
}

impl Default for CompositeTcpListener {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtualIoSource for CompositeTcpListener {
    fn remove_handler(&mut self) {
        for port in self.ports.iter_mut() {
            port.remove_handler();
        }
    }

    fn poll_read_ready(&mut self, cx: &mut Context<'_>) -> Poll<crate::Result<usize>> {
        for port in self.ports.iter_mut() {
            if let Poll::Ready(ready) = port.poll_read_ready(cx) {
                return Poll::Ready(ready);
            }
        }
        Poll::Pending
    }

    fn poll_write_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<usize>> {
        for port in self.ports.iter_mut() {
            if let Poll::Ready(ready) = port.poll_write_ready(cx) {
                return Poll::Ready(ready);
            }
        }
        Poll::Pending
    }
}

impl VirtualTcpListener for CompositeTcpListener {
    fn try_accept(
        &mut self,
    ) -> crate::Result<(Box<dyn crate::VirtualTcpSocket + Sync>, SocketAddr)> {
        let mut ret = NetworkError::Unsupported;
        for port in self.ports.iter_mut() {
            match port.try_accept() {
                Ok(ret) => return Ok(ret),
                Err(err) => {
                    ret = err;
                }
            }
        }
        Err(ret)
    }

    fn set_handler(
        &mut self,
        handler: Box<dyn crate::InterestHandler + Send + Sync>,
    ) -> crate::Result<()> {
        let handler = ArcInterestHandler::new(handler);
        for port in self.ports.iter_mut() {
            port.set_handler(Box::new(handler.clone()))?;
        }
        Ok(())
    }

    fn addr_local(&self) -> crate::Result<SocketAddr> {
        if self.ports.len() > 1 {
            let addr = self.ports.first().unwrap().addr_local()?;
            if addr.is_ipv4() {
                Ok(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), addr.port()))
            } else {
                Ok(SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), addr.port()))
            }
        } else if let Some(addr) = self.ports.first() {
            addr.addr_local()
        } else {
            Err(NetworkError::Unsupported)
        }
    }

    fn set_ttl(&mut self, ttl: u8) -> crate::Result<()> {
        for port in self.ports.iter_mut() {
            match port.set_ttl(ttl) {
                Ok(()) | Err(NetworkError::Unsupported) => {}
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }

    fn ttl(&self) -> crate::Result<u8> {
        let mut ret = u8::MAX;
        for port in self.ports.iter() {
            if let Ok(ttl) = port.ttl() {
                ret = ret.min(ttl)
            }
        }
        Ok(ret)
    }
}
