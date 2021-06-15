use super::c::*;
use crate::{abi::*, types::*};
use std::convert::TryInto;
use std::io;
use std::net::{Shutdown, SocketAddr, SocketAddrV4, ToSocketAddrs};
use std::ptr::NonNull;

pub struct TcpListener {
    fd: __wasi_fd_t,
    address: SocketAddr,
}

impl TcpListener {
    fn new(address: SocketAddr) -> io::Result<Self> {
        let mut fd: __wasi_fd_t = 0;

        unsafe { socket_create(&mut fd, AF_INET, SOCK_STREAM, DEFAULT_PROTOCOL) }.into_result()?;

        match address {
            SocketAddr::V4(v4) => {
                let address = SockaddrIn::from(&v4);

                unsafe {
                    socket_bind(
                        fd,
                        NonNull::new_unchecked(&address as *const _ as *mut _),
                        address.size_of_self(),
                    )
                }
                .into_result()?;
            }

            SocketAddr::V6(_v6) => todo!("V6 not implemented"),
        };

        unsafe { socket_listen(fd, 128) }.into_result()?;

        Ok(TcpListener { fd, address })
    }

    pub fn bind<A: ToSocketAddrs>(addresses: A) -> io::Result<Self> {
        let addresses = addresses.to_socket_addrs()?;

        for address in addresses {
            match Self::new(address) {
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

    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        unsafe {
            socket_shutdown(
                self.fd,
                match how {
                    Shutdown::Read => SHUT_RD,
                    Shutdown::Write => SHUT_WR,
                    Shutdown::Both => SHUT_RDWR,
                },
            )
        }
        .into_result()?;

        Ok(())
    }
}

impl io::Read for TcpStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let io_vec = vec![__wasi_ciovec_t {
            buf: (buffer.as_mut_ptr() as usize).try_into().unwrap(),
            buf_len: buffer.len().try_into().unwrap(),
        }];
        let mut io_read = 0;

        unsafe {
            socket_recv(
                self.fd,
                NonNull::new_unchecked(io_vec.as_ptr() as *const _ as *mut _),
                io_vec.len() as u32,
                0,
                &mut io_read,
            )
        }
        .into_result()?;

        Ok(io_read as usize)
    }
}

impl io::Write for TcpStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let io_vec = vec![__wasi_ciovec_t {
            buf: (buffer.as_ptr() as usize).try_into().unwrap(),
            buf_len: buffer.len().try_into().unwrap(),
        }];
        let mut io_written = 0;

        unsafe {
            socket_send(
                self.fd,
                NonNull::new_unchecked(io_vec.as_ptr() as *const _ as *mut _),
                io_vec.len() as u32,
                0,
                &mut io_written,
            )
        }
        .into_result()?;

        Ok(io_written as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
