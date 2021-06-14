use std::convert::TryInto;
use std::error;
use std::fmt;
use std::io;
use wasmer::{Exports, Function, Instance, Module, Store};
use wasmer_wasi::{
    ptr::{Array, WasmPtr},
    WasiEnv, WasiState,
};
use wasmer_wasi_experimental_network::types::*;

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
    println!("# socket_create");

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
        panic!("`socket_create` failed with `{:?}`", Error::current());
    }

    let (memory, _) = env.get_memory_and_wasi_state(0);
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
    println!("# socket_bind");

    let (memory, _) = env.get_memory_and_wasi_state(0);

    let address_offset = address.offset() as usize;

    if address_offset + (address_size as usize) > memory.size().bytes().0 || address_size == 0 {
        panic!("Failed to map `address` to something inside the memory");
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
        panic!("`socket_bind` failed with `{:?}`", Error::current());
    }

    __WASI_ESUCCESS
}

fn socket_listen(fd: __wasi_fd_t, backlog: u32) -> __wasi_errno_t {
    println!("# socket_listen");

    let err = unsafe { libc::listen(fd.try_into().unwrap(), backlog.try_into().unwrap()) };

    if err != 0 {
        panic!("`socket_listen` failed with `{:?}`", Error::current());
    }

    __WASI_ESUCCESS
}

fn socket_accept(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    address: WasmPtr<u32>,
    address_size: WasmPtr<u32>,
    remote_fd: WasmPtr<__wasi_fd_t>,
) -> __wasi_errno_t {
    println!("# socket_accept");

    let (memory, _) = env.get_memory_and_wasi_state(0);

    let address_size_cell = wasi_try!(address_size.deref(memory));
    let address_size = address_size_cell.get() as usize;

    let address_offset = address.offset() as usize;

    if address_offset + address_size > memory.size().bytes().0 || address_size == 0 {
        panic!("Failed to map `address` to something inside the memory");
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
        panic!("`socket_accept` failed with `{:?}`", Error::current());
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
    println!("# socket_send");

    let (memory, _) = env.get_memory_and_wasi_state(0);

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
            panic!("`socket_send` failed with `{:?}`", Error::current());
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
    println!("# socket_recv");

    let (memory, _) = env.get_memory_and_wasi_state(0);

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
            panic!("`socket_read` failed with `{:?}`", Error::current());
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
        panic!("`socket_shutdown` failed with `{:?}`", Error::current());
    }

    __WASI_ESUCCESS
}

fn main() -> Result<(), Box<dyn error::Error>> {
    let store = Store::default();
    let module = Module::from_file(
        &store,
        "../../../target/wasm32-wasi/release/examples/tcp_server.wasm",
    )?;

    let mut wasi_env = WasiState::new("tcp-server").finalize()?;
    let mut import_object = wasi_env.import_object(&module)?;

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

    import_object.register("wasi_experimental_network_unstable", wasi_network_imports);

    let instance = Instance::new(&module, &import_object)?;
    let _results = instance.exports.get_function("_start")?.call(&[])?;

    Ok(())
}
