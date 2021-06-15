use crate::blocking::types::*;
use std::convert::TryInto;
use std::fmt;
use std::io;
use wasmer::{Exports, Function, Store};
use wasmer_wasi::{
    ptr::{Array, WasmPtr},
    WasiEnv,
};

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

struct Error {
    inner: io::Error,
}

impl Error {
    fn current() -> Self {
        Self {
            inner: io::Error::last_os_error(),
        }
    }

    fn wasi_errno(&self) -> __wasi_errno_t {
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

fn socket_create(
    env: &WasiEnv,
    fd_out: WasmPtr<__wasi_fd_t>,
    domain: __wasi_socket_domain_t,
    r#type: __wasi_socket_type_t,
    protocol: __wasi_socket_protocol_t,
) -> __wasi_errno_t {
    let domain = match domain {
        AF_INET => libc::AF_INET,
        AF_INET6 => libc::AF_INET6,
        d => {
            eprintln!("Unkown domain `{}`", d);
            return __WASI_EINVAL;
        }
    };
    let r#type = match r#type {
        SOCK_STREAM => libc::SOCK_STREAM,
        SOCK_DGRAM => libc::SOCK_DGRAM,
        t => {
            eprintln!("Unknown type `{}`", t);
            return __WASI_EINVAL;
        }
    };
    let protocol = protocol as i32;

    let new_fd = unsafe { libc::socket(domain, r#type, protocol) };

    if new_fd < 0 {
        return Error::current().wasi_errno();
    }

    let memory = env.memory();
    let fd_out_cell = wasi_try!(fd_out.deref(memory));
    fd_out_cell.set(new_fd.try_into().unwrap());

    __WASI_ESUCCESS
}

fn socket_bind(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    address: WasmPtr<u32>,
    address_size: u32,
) -> __wasi_errno_t {
    let memory = env.memory();

    let address_offset = address.offset() as usize;

    if address_offset + (address_size as usize) > memory.size().bytes().0 || address_size == 0 {
        eprintln!("Failed to map `address` to something inside the memory");
        return __WASI_EINVAL;
    }

    let address_ptr: *mut u8 = unsafe { memory.data_ptr().add(address_offset) };

    let err = unsafe {
        libc::bind(
            fd.try_into().unwrap(),
            address_ptr as *const _,
            address_size,
        )
    };

    if err != 0 {
        Error::current().wasi_errno()
    } else {
        __WASI_ESUCCESS
    }
}

fn socket_listen(fd: __wasi_fd_t, backlog: u32) -> __wasi_errno_t {
    let err = unsafe { libc::listen(fd.try_into().unwrap(), backlog.try_into().unwrap()) };

    if err != 0 {
        Error::current().wasi_errno()
    } else {
        __WASI_ESUCCESS
    }
}

fn socket_accept(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    address: WasmPtr<u32>,
    address_size: WasmPtr<u32>,
    remote_fd: WasmPtr<__wasi_fd_t>,
) -> __wasi_errno_t {
    let memory = env.memory();

    let address_size_cell = wasi_try!(address_size.deref(memory));
    let address_size = address_size_cell.get() as usize;

    let address_offset = address.offset() as usize;

    if address_offset + address_size > memory.size().bytes().0 || address_size == 0 {
        eprintln!("Failed to map `address` to something inside the memory");
        return __WASI_EINVAL;
    }

    let address_ptr: *mut u8 = unsafe { memory.data_ptr().add(address_offset) };

    let new_fd = unsafe {
        libc::accept(
            fd.try_into().unwrap(),
            address_ptr as *mut _,
            address_size_cell.as_ptr(),
        )
    };

    if new_fd < 0 {
        return Error::current().wasi_errno();
    }

    let remote_fd_cell = wasi_try!(remote_fd.deref(memory));
    remote_fd_cell.set(new_fd.try_into().unwrap());

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
    let memory = env.memory();

    let iov = wasi_try!(iov.deref(memory, 0, iov_size));

    let mut total_bytes_written: u32 = 0;

    for iov_cell in iov {
        let iov_inner = iov_cell.get();
        let bytes =
            wasi_try!(WasmPtr::<u8, Array>::new(iov_inner.buf).deref(memory, 0, iov_inner.buf_len));
        let buffer: &mut [u8] = unsafe { &mut *(bytes as *const [_] as *mut [_] as *mut [u8]) };

        let written_bytes = unsafe {
            libc::send(
                fd.try_into().unwrap(),
                buffer.as_ptr() as *mut _,
                buffer.len(),
                iov_flags.into(),
            )
        };

        if written_bytes < 0 {
            return Error::current().wasi_errno();
        }

        total_bytes_written += written_bytes as u32;
    }

    let io_size_out_cell = wasi_try!(io_size_out.deref(memory));
    io_size_out_cell.set(total_bytes_written);

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
    let memory = env.memory();

    let iov = wasi_try!(iov.deref(memory, 0, iov_size));

    let mut total_bytes_read: u32 = 0;

    for iov_cell in iov {
        let iov_inner = iov_cell.get();
        let bytes =
            wasi_try!(WasmPtr::<u8, Array>::new(iov_inner.buf).deref(memory, 0, iov_inner.buf_len));
        let buffer: &mut [u8] = unsafe { &mut *(bytes as *const [_] as *mut [_] as *mut [u8]) };

        let read_bytes = unsafe {
            libc::recv(
                fd.try_into().unwrap(),
                buffer.as_ptr() as *mut _,
                buffer.len(),
                iov_flags.into(),
            )
        };

        if read_bytes < 0 {
            return Error::current().wasi_errno();
        }

        total_bytes_read += read_bytes as u32;
    }

    let io_size_out_cell = wasi_try!(io_size_out.deref(memory));
    io_size_out_cell.set(total_bytes_read);

    __WASI_ESUCCESS
}

fn socket_shutdown(fd: __wasi_fd_t, how: __wasi_shutdown_t) -> __wasi_errno_t {
    let how = match how {
        SHUT_RD => libc::SHUT_RD,
        SHUT_WR => libc::SHUT_WR,
        SHUT_RDWR => libc::SHUT_RDWR,
        s => {
            eprintln!("Unkown shutdown constant `{}`", s);
            return __WASI_EINVAL;
        }
    };
    let err = unsafe { libc::shutdown(fd.try_into().unwrap(), how) };

    if err != 0 {
        Error::current().wasi_errno()
    } else {
        __WASI_ESUCCESS
    }
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
