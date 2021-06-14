use std::convert::TryInto;
use std::error::Error;
use std::ptr;
use wasmer::{Exports, Function, Instance, Module, Store};
use wasmer_wasi::{ptr::WasmPtr, WasiEnv, WasiState};
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
        panic!("`socket_create` failed with `{}`", new_fd);
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
        panic!("`socket_bind` failed with `{}`", err);
    }

    __WASI_ESUCCESS
}

fn socket_listen(fd: __wasi_fd_t, backlog: u32) -> __wasi_errno_t {
    let err = unsafe { libc::listen(fd.try_into().unwrap(), backlog.try_into().unwrap()) };

    if err != 0 {
        panic!("`socket_listen` failed with `{}`", err);
    }

    __WASI_ESUCCESS
}

fn socket_accept(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    _address: u32,
    _address_size: u32,
    fd_out: WasmPtr<__wasi_fd_t>,
) -> __wasi_errno_t {
    let new_fd = unsafe { libc::accept(fd.try_into().unwrap(), ptr::null_mut(), ptr::null_mut()) };

    // TODO: Support `address` and `address_size`

    if new_fd < 0 {
        panic!("`socket_accept` failed with `{}`", new_fd);
    }

    let (memory, _) = env.get_memory_and_wasi_state(0);
    let fd_out_cell = wasi_try!(fd_out.deref(memory));
    fd_out_cell.set(new_fd.try_into().unwrap());

    __WASI_ESUCCESS
}

fn socket_recv(
    _fd: u32,
    _iov: u32,
    _iov_size: u32,
    _iov_flags: u32,
    _io_size_out: u32,
) -> __wasi_errno_t {
    todo!("socket_recv")
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("yolo");

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
    wasi_network_imports.insert("socket_recv", Function::new_native(&store, socket_recv));

    import_object.register("wasi_experimental_network_unstable", wasi_network_imports);

    let instance = Instance::new(&module, &import_object)?;
    let results = instance.exports.get_function("_start")?.call(&[])?;

    dbg!(results);

    Ok(())
}
