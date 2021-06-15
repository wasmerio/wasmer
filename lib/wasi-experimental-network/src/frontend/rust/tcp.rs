use super::c::*;
use crate::{abi::*, types::*};
use std::ffi::c_void;
use std::io;
use std::net::{SocketAddr, SocketAddrV4, ToSocketAddrs};
use std::ptr::NonNull;

pub struct TcpListener {
    fd: __wasi_fd_t,
    address: SocketAddr,
}

impl TcpListener {
    fn raw_bind(address: SocketAddr) -> io::Result<Self> {
        let mut fd: __wasi_fd_t = 0;

        unsafe { socket_create(&mut fd, AF_INET, SOCK_STREAM, DEFAULT_PROTOCOL) }.into_result()?;

        let (lower_address, lower_address_size): (*const c_void, u32) = match address {
            SocketAddr::V4(v4) => {
                let c_address = SockaddrIn::from(&v4);

                (&c_address as *const _ as *const _, c_address.size_of_self())
            }

            SocketAddr::V6(_v6) => todo!("V6 not implemented"),
        };

        unsafe {
            socket_bind(
                fd,
                NonNull::new_unchecked(lower_address as *mut _),
                lower_address_size,
            )
        }
        .into_result()?;

        unsafe { socket_listen(fd, 128) }.into_result()?;

        Ok(TcpListener { fd, address })
    }

    pub fn bind<A: ToSocketAddrs>(addresses: A) -> io::Result<Self> {
        let addresses = addresses.to_socket_addrs()?;

        for address in addresses {
            match Self::raw_bind(address) {
                Ok(listener) => return Ok(listener),
                Err(_) => continue,
            }
        }

        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "could not resolve to any addresses",
        ))
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.address)
    }

    pub fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        let mut remote_fd: __wasi_fd_t = 0;
        let mut remote_address = SockaddrIn::default();
        let mut remote_address_size = remote_address.size_of_self();

        unsafe {
            socket_accept(
                self.fd,
                &mut remote_address as *mut _ as *mut u8,
                &mut remote_address_size,
                &mut remote_fd,
            )
        }
        .into_result()?;

        let remote_address = SocketAddr::V4(Into::<SocketAddrV4>::into(remote_address));

        Ok((TcpStream::new(remote_fd, remote_address), remote_address))
    }

    pub fn incoming(&self) -> Incoming<'_> {
        Incoming { listener: self }
    }
}

pub struct Incoming<'a> {
    listener: &'a TcpListener,
}

impl<'a> Iterator for Incoming<'a> {
    type Item = io::Result<TcpStream>;

    fn next(&mut self) -> Option<io::Result<TcpStream>> {
        Some(self.listener.accept().map(|(stream, _)| stream))
    }
}

pub struct TcpStream {
    fd: __wasi_fd_t,
    address: SocketAddr,
}

impl TcpStream {
    fn new(fd: __wasi_fd_t, address: SocketAddr) -> Self {
        Self { fd, address }
    }
}
