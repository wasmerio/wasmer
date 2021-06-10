use std::ptr::NonNull;
use wasmer_wasi_experimental_network::{abi::*, types::*};

fn main() {
    println!("Creating the socket");

    let fd = {
        let mut fd: __wasi_fd_t = 0;
        let err = unsafe { socket_create(&mut fd, AF_INET, SOCK_STREAM, DEFAULT_PROTOCOL) };

        if err != 0 {
            panic!("`socket_create` failed with `{}`", err);
        }

        fd
    };

    println!("Binding the socket");

    let address = SocketAddress {
        sin_family: AF_INET as _,
        sin_port: 9000u16.to_be(),
        sin_addr: [0; 4],
        sin_zero: [0; 8],
    };

    let err = unsafe {
        socket_bind(
            fd,
            NonNull::new_unchecked(&address as *const _ as *mut _),
            address.size_of_self(),
        )
    };

    if err != 0 {
        panic!("`socket_bind` failed with `{}`", err);
    }

    println!("Listening");

    let err = unsafe { socket_listen(fd) };

    if err != 0 {
        panic!("`socket_listen` failed with `{}`", err);
    }

    println!("Accepting new connection");

    let mut client_fd: __wasi_fd_t = 0;
    let mut client_address = SocketAddress::default();
    let mut client_address_size = 0;
    let err = unsafe {
        socket_accept(
            fd,
            &mut client_address as *mut _ as *mut u8,
            &mut client_address_size,
            &mut client_fd,
        )
    };

    if err != 0 {
        panic!("`socket_accept` failed with `{}`", err);
    }
}
