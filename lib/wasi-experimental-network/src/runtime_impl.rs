use crate::types::*;
use socket2 as socket;
use std::cell::Cell;
use std::convert::TryInto;
use std::io;
use std::mem;
use std::net;
use wasmer::{Exports, Function, Store};
use wasmer_wasi::{
    ptr::{Array, WasmPtr},
    WasiEnv,
};

trait TryFrom<T>: Sized {
    type Error;

    fn try_from(value: T) -> Result<Self, Self::Error>;
}

/*
trait TryInto<T>: Sized {
    type Error;

    fn try_into(self) -> Result<T, Self::Error>;
}

impl TryInto<T, U> for T
where
    U: TryFrom<T>,
{
    type Error = U::Error;

    fn try_into(self) -> Result<U, U::Error> {
        U::try_from(self)
    }
}
*/

impl TryFrom<__wasi_socket_type_t> for socket::Type {
    type Error = String;

    fn try_from(value: __wasi_socket_type_t) -> Result<Self, Self::Error> {
        Ok(match value {
            SOCK_STREAM => Self::STREAM,
            SOCK_DGRAM => Self::DGRAM,
            SOCK_SEQPACKET => Self::SEQPACKET,
            #[cfg(not(target_os = "redox"))]
            SOCK_RAW => Self::RAW,
            t => return Err(format!("Unknown socket type `{}`", t)),
        })
    }
}

impl TryFrom<__wasi_socket_domain_t> for socket::Domain {
    type Error = String;

    fn try_from(value: __wasi_socket_domain_t) -> Result<Self, Self::Error> {
        Ok(match value {
            AF_INET => Self::IPV4,
            AF_INET6 => Self::IPV6,
            #[cfg(target_family = "unix")]
            AF_UNIX => Self::UNIX,
            #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
            AF_PACKET => Self::PACKET,
            #[cfg(any(target_os = "android", target_os = "linux"))]
            AF_VSOCK => Self::VSOCK,
            d => return Err(format!("Unknown socket domain `{}`", d)),
        })
    }
}

impl TryFrom<__wasi_socket_protocol_t> for Option<socket::Protocol> {
    type Error = String;

    fn try_from(value: __wasi_socket_protocol_t) -> Result<Self, Self::Error> {
        #![allow(non_upper_case_globals)]

        Ok(match value {
            DEFAULT_PROTOCOL => None,
            ICMPv4 => Some(socket::Protocol::ICMPV4),
            ICMPv6 => Some(socket::Protocol::ICMPV6),
            TCP => Some(socket::Protocol::TCP),
            UDP => Some(socket::Protocol::UDP),
            d => return Err(format!("Unknown socket protocol `{}`", d)),
        })
    }
}

impl TryFrom<__wasi_shutdown_t> for net::Shutdown {
    type Error = String;

    fn try_from(value: __wasi_shutdown_t) -> Result<Self, Self::Error> {
        Ok(match value {
            SHUT_RD => Self::Read,
            SHUT_WR => Self::Write,
            SHUT_RDWR => Self::Both,
            s => return Err(format!("Unknown shutdown option `{}`", s)),
        })
    }
}

impl TryFrom<__wasi_socket_address_t> for net::SocketAddr {
    type Error = String;

    fn try_from(value: __wasi_socket_address_t) -> Result<Self, Self::Error> {
        Ok(unsafe {
            match value {
                __wasi_socket_address_t {
                    v4:
                        __wasi_socket_address_in_t {
                            family: AF_INET,
                            address,
                            port,
                        },
                } => {
                    let [o, p, q, r] = address;

                    net::SocketAddr::V4(net::SocketAddrV4::new(
                        net::Ipv4Addr::new(o, p, q, r),
                        u16::from_be(port),
                    ))
                }
                _ => return Err(format!("IPv6 not supported for the moment")),
            }
        })
    }
}

impl TryFrom<Option<net::SocketAddr>> for __wasi_socket_address_t {
    type Error = String;

    fn try_from(value: Option<net::SocketAddr>) -> Result<Self, Self::Error> {
        Ok(match value {
            Some(net::SocketAddr::V4(v4)) => __wasi_socket_address_t {
                v4: __wasi_socket_address_in_t {
                    family: AF_INET,
                    address: v4.ip().octets(),
                    port: v4.port().to_be(),
                },
            },
            _ => return Err(format!("IPv6 not supported for the moment")),
        })
    }
}

trait AsFd {
    fn as_fd(&self) -> u32;
}

trait FromFd {
    unsafe fn from_fd(fd: u32) -> Self;
}

#[cfg(target_family = "unix")]
impl<T> AsFd for T
where
    T: std::os::unix::io::AsRawFd,
{
    fn as_fd(&self) -> u32 {
        self.as_raw_fd().try_into().unwrap()
    }
}

#[cfg(target_family = "windows")]
impl<T> AsFd for T
where
    T: std::os::windows::io::AsRawSocket,
{
    fn as_fd(&self) -> u32 {
        self.as_raw_socket().try_into().unwrap()
    }
}

#[cfg(target_family = "unix")]
impl FromFd for socket::Socket {
    unsafe fn from_fd(fd: u32) -> Self {
        std::os::unix::io::FromRawFd::from_raw_fd(fd.try_into().unwrap())
    }
}

#[cfg(target_family = "windows")]
impl FromFd for socket::Socket {
    unsafe fn from_fd(fd: u32) -> Self {
        std::os::unix::io::FromRawSocket::from_raw_socket(fd.try_into().unwrap())
    }
}

macro_rules! wasi_try {
    ($expr:expr) => {{
        let res: Result<_, __wasi_errno_t> = $expr;

        match res {
            Ok(val) => val,
            Err(err) => return err,
        }
    }};

    ($expr:expr, $e:expr) => {{
        let opt: Option<_> = $expr;
        wasi_try!(opt.ok_or($e))
    }};
}

/*
pub(crate) struct Error {
    inner: io::Error,
}

impl Error {
    pub(crate) fn current() -> Self {
        Self {
            inner: io::Error::last_os_error(),
        }
    }

    pub(crate) fn wasi_errno(&self) -> __wasi_errno_t {
        // SAFETY: We can unwrap here because the error has been
        // constructed with `Error::last_os_error`.
        match self.inner.raw_os_error().unwrap() {
            libc::E2BIG => __WASI_E2BIG,
            libc::EACCES => __WASI_EACCES,
            libc::EADDRINUSE => __WASI_EADDRINUSE,
            libc::EADDRNOTAVAIL => __WASI_EADDRNOTAVAIL,
            libc::EAFNOSUPPORT => __WASI_EAFNOSUPPORT,
            libc::EAGAIN => __WASI_EAGAIN,
            libc::EALREADY => __WASI_EALREADY,
            libc::EBADF => __WASI_EBADF,
            libc::EBADMSG => __WASI_EBADMSG,
            libc::EBUSY => __WASI_EBUSY,
            libc::ECANCELED => __WASI_ECANCELED,
            libc::ECHILD => __WASI_ECHILD,
            libc::ECONNABORTED => __WASI_ECONNABORTED,
            libc::ECONNREFUSED => __WASI_ECONNREFUSED,
            libc::ECONNRESET => __WASI_ECONNRESET,
            libc::EDEADLK => __WASI_EDEADLK,
            libc::EDESTADDRREQ => __WASI_EDESTADDRREQ,
            libc::EDOM => __WASI_EDOM,
            libc::EDQUOT => __WASI_EDQUOT,
            libc::EEXIST => __WASI_EEXIST,
            libc::EFAULT => __WASI_EFAULT,
            libc::EFBIG => __WASI_EFBIG,
            libc::EHOSTUNREACH => __WASI_EHOSTUNREACH,
            libc::EIDRM => __WASI_EIDRM,
            libc::EILSEQ => __WASI_EILSEQ,
            libc::EINPROGRESS => __WASI_EINPROGRESS,
            libc::EINTR => __WASI_EINTR,
            libc::EINVAL => __WASI_EINVAL,
            libc::EIO => __WASI_EIO,
            libc::EISCONN => __WASI_EISCONN,
            libc::EISDIR => __WASI_EISDIR,
            libc::ELOOP => __WASI_ELOOP,
            libc::EMFILE => __WASI_EMFILE,
            libc::EMLINK => __WASI_EMLINK,
            libc::EMSGSIZE => __WASI_EMSGSIZE,
            libc::EMULTIHOP => __WASI_EMULTIHOP,
            libc::ENAMETOOLONG => __WASI_ENAMETOOLONG,
            libc::ENETDOWN => __WASI_ENETDOWN,
            libc::ENETRESET => __WASI_ENETRESET,
            libc::ENETUNREACH => __WASI_ENETUNREACH,
            libc::ENFILE => __WASI_ENFILE,
            libc::ENOBUFS => __WASI_ENOBUFS,
            libc::ENODEV => __WASI_ENODEV,
            libc::ENOENT => __WASI_ENOENT,
            libc::ENOEXEC => __WASI_ENOEXEC,
            libc::ENOLCK => __WASI_ENOLCK,
            libc::ENOLINK => __WASI_ENOLINK,
            libc::ENOMEM => __WASI_ENOMEM,
            libc::ENOMSG => __WASI_ENOMSG,
            libc::ENOPROTOOPT => __WASI_ENOPROTOOPT,
            libc::ENOSPC => __WASI_ENOSPC,
            libc::ENOSYS => __WASI_ENOSYS,
            libc::ENOTCONN => __WASI_ENOTCONN,
            libc::ENOTDIR => __WASI_ENOTDIR,
            libc::ENOTEMPTY => __WASI_ENOTEMPTY,
            libc::ENOTRECOVERABLE => __WASI_ENOTRECOVERABLE,
            libc::ENOTSOCK => __WASI_ENOTSOCK,
            libc::ENOTSUP => __WASI_ENOTSUP,
            libc::ENOTTY => __WASI_ENOTTY,
            libc::ENXIO => __WASI_ENXIO,
            libc::EOVERFLOW => __WASI_EOVERFLOW,
            libc::EOWNERDEAD => __WASI_EOWNERDEAD,
            libc::EPERM => __WASI_EPERM,
            libc::EPIPE => __WASI_EPIPE,
            libc::EPROTO => __WASI_EPROTO,
            libc::EPROTONOSUPPORT => __WASI_EPROTONOSUPPORT,
            libc::EPROTOTYPE => __WASI_EPROTOTYPE,
            libc::ERANGE => __WASI_ERANGE,
            libc::EROFS => __WASI_EROFS,
            libc::ESPIPE => __WASI_ESPIPE,
            libc::ESRCH => __WASI_ESRCH,
            libc::ESTALE => __WASI_ESTALE,
            libc::ETIMEDOUT => __WASI_ETIMEDOUT,
            libc::ETXTBSY => __WASI_ETXTBSY,
            libc::EXDEV => __WASI_EXDEV,
            errno => panic!("Unknown error {}", errno),
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_fmt(format_args!("{:?}", self.inner))
    }
}
*/

fn socket_create(
    env: &WasiEnv,
    domain: __wasi_socket_domain_t,
    r#type: __wasi_socket_type_t,
    protocol: __wasi_socket_protocol_t,
    fd_out: WasmPtr<__wasi_fd_t>,
) -> __wasi_errno_t {
    let domain = socket::Domain::try_from(domain).unwrap();
    let r#type = socket::Type::try_from(r#type).unwrap();
    let protocol = Option::<socket::Protocol>::try_from(protocol).unwrap();

    let socket = socket::Socket::new(domain, r#type, protocol).unwrap();
    let fd = socket.as_fd();

    let memory = env.memory();
    let fd_out_cell = wasi_try!(fd_out.deref(memory));
    fd_out_cell.set(fd);

    // Do not drop/close the socket.
    mem::forget(socket);

    __WASI_ESUCCESS
}

fn socket_bind(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    address: WasmPtr<__wasi_socket_address_t>,
) -> __wasi_errno_t {
    let memory = env.memory();
    let address = wasi_try!(address.deref(memory)).get();

    let socket_address = net::SocketAddr::try_from(address).unwrap();
    let socket_address = socket::SockAddr::from(socket_address);

    let socket = unsafe { socket::Socket::from_fd(fd) };
    socket.bind(&socket_address).unwrap();

    // Do not drop/close the socket.
    mem::forget(socket);

    __WASI_ESUCCESS
}

fn socket_listen(fd: __wasi_fd_t, backlog: u32) -> __wasi_errno_t {
    let socket = unsafe { socket::Socket::from_fd(fd) };
    socket.listen(backlog.try_into().unwrap()).unwrap();

    mem::forget(socket);

    __WASI_ESUCCESS
}

fn socket_accept(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    remote_address: WasmPtr<__wasi_socket_address_t>,
    remote_fd: WasmPtr<__wasi_fd_t>,
) -> __wasi_errno_t {
    let socket = unsafe { socket::Socket::from_fd(fd) };
    let (remote_socket, remote_socket_address) = socket.accept().unwrap();

    // Do not drop/close the sockets.
    mem::forget(socket);

    let memory = env.memory();
    let remote_address_cell = wasi_try!(remote_address.deref(memory));
    remote_address_cell
        .set(__wasi_socket_address_t::try_from(remote_socket_address.as_socket()).unwrap());

    let remote_fd_cell = wasi_try!(remote_fd.deref(memory));
    remote_fd_cell.set(remote_socket.as_fd());

    mem::forget(remote_socket);

    __WASI_ESUCCESS
}

fn socket_send(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    iov: WasmPtr<__wasi_ciovec_t, Array>,
    iov_size: u32,
    iov_flags: __wasi_siflags_t,
    io_size_out: WasmPtr<u32>,
) -> __wasi_errno_t {
    let socket = unsafe { socket::Socket::from_fd(fd) };

    let memory = env.memory();
    let io_slices = wasi_try!(iov.deref(memory, 0, iov_size))
        .iter()
        .map(|iov_cell| {
            let iov_inner = iov_cell.get();
            let bytes: &[Cell<u8>] =
                WasmPtr::<u8, Array>::new(iov_inner.buf).deref(memory, 0, iov_inner.buf_len)?;
            let bytes: &[u8] = unsafe { mem::transmute(bytes) };

            Ok(io::IoSlice::new(bytes))
        })
        .collect::<Result<Vec<_>, __wasi_errno_t>>()
        .unwrap();

    let total_bytes_written = socket
        .send_vectored_with_flags(&io_slices, iov_flags.try_into().unwrap())
        .unwrap();

    // Do not drop/close the socket.
    mem::forget(socket);

    let io_size_out_cell = wasi_try!(io_size_out.deref(memory));
    io_size_out_cell.set(total_bytes_written.try_into().unwrap());

    __WASI_ESUCCESS
}

fn socket_recv(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    iov: WasmPtr<__wasi_ciovec_t, Array>,
    iov_size: u32,
    iov_flags: __wasi_siflags_t,
    io_size_out: WasmPtr<u32>,
) -> __wasi_errno_t {
    let socket = unsafe { socket::Socket::from_fd(fd) };

    let memory = env.memory();
    let mut slices: Vec<&[mem::MaybeUninit<u8>]> = Vec::with_capacity(iov_size.try_into().unwrap());

    for iov_cell in wasi_try!(iov.deref(memory, 0, iov_size)).iter() {
        let iov_inner = iov_cell.get();
        let bytes: &[Cell<u8>] =
            wasi_try!(WasmPtr::<u8, Array>::new(iov_inner.buf).deref(memory, 0, iov_inner.buf_len));
        let bytes: &[mem::MaybeUninit<u8>] = unsafe {
            &*(bytes as *const [Cell<u8>] as *const [u8] as *const [mem::MaybeUninit<u8>])
        };

        slices.push(bytes);
    }

    let mut io_slices: Vec<socket::MaybeUninitSlice> =
        Vec::with_capacity(iov_size.try_into().unwrap());

    for slice in slices {
        io_slices.push(socket::MaybeUninitSlice::new(unsafe {
            &mut *(slice as *const [_] as *mut [_])
        }));
    }

    let (total_bytes_read, _recv_flags) = socket
        .recv_vectored_with_flags(&mut io_slices, iov_flags.try_into().unwrap())
        .unwrap();

    // Do not drop/close the socket.
    mem::forget(socket);

    let io_size_out_cell = wasi_try!(io_size_out.deref(memory));
    io_size_out_cell.set(total_bytes_read.try_into().unwrap());

    __WASI_ESUCCESS
}

fn socket_shutdown(fd: __wasi_fd_t, how: __wasi_shutdown_t) -> __wasi_errno_t {
    let how = net::Shutdown::try_from(how).unwrap();
    let socket = unsafe { socket::Socket::from_fd(fd) };
    socket.shutdown(how).unwrap();

    // Do not drop/close the socket.
    mem::forget(socket);

    __WASI_ESUCCESS
}

pub fn get_namespace(store: &Store, wasi_env: &WasiEnv) -> (&'static str, Exports) {
    let mut wasi_network_imports = Exports::new();
    wasi_network_imports.insert(
        "socket_create",
        Function::new_native_with_env(&store, wasi_env.clone(), socket_create),
    );
    wasi_network_imports.insert(
        "socket_bind",
        Function::new_native_with_env(&store, wasi_env.clone(), socket_bind),
    );
    wasi_network_imports.insert("socket_listen", Function::new_native(&store, socket_listen));
    wasi_network_imports.insert(
        "socket_accept",
        Function::new_native_with_env(&store, wasi_env.clone(), socket_accept),
    );
    wasi_network_imports.insert(
        "socket_send",
        Function::new_native_with_env(&store, wasi_env.clone(), socket_send),
    );
    wasi_network_imports.insert(
        "socket_recv",
        Function::new_native_with_env(&store, wasi_env.clone(), socket_recv),
    );
    wasi_network_imports.insert(
        "socket_shutdown",
        Function::new_native(&store, socket_shutdown),
    );

    ("wasi_experimental_network_unstable", wasi_network_imports)
}
