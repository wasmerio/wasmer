use std::ptr::NonNull;
use wasmer_wasi_experimental_network::{abi::*, types::*};

fn main() {
    println!("Creating the socket");

    let fd = {
        let mut fd: __wasi_fd_t = 0;
        let err = unsafe { socket_create(&mut fd, AF_INET, SOCK_STREAM, DEFAULT_PROTOCOL) };

        if err != __WASI_ESUCCESS {
            panic!("`socket_create` failed with `{}`", err);
        }

        fd
    };

    println!("Binding the socket");

    let address = SockaddrIn {
        sin_family: AF_INET as _,
        sin_port: 9002u16.to_be(),
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

    if err != __WASI_ESUCCESS {
        panic!("`socket_bind` failed with `{}`", err);
    }

    println!("Listening");

    let err = unsafe { socket_listen(fd, 3) };

    if err != __WASI_ESUCCESS {
        panic!("`socket_listen` failed with `{}`", err);
    }

    loop {
        println!("Waiting to accept a new connection");

        let mut client_fd: __wasi_fd_t = 0;
        let mut client_address = SockaddrIn::default();
        let mut client_address_size = client_address.size_of_self();
        let err = unsafe {
            socket_accept(
                fd,
                &mut client_address as *mut _ as *mut u8,
                &mut client_address_size,
                &mut client_fd,
            )
        };

        println!("Remote client IP: `{:?}`", &client_address);

        if err != __WASI_ESUCCESS {
            panic!("`socket_accept` failed with `{}`", err);
        }

        let mut buffer: Vec<u8> = vec![0; 128];
        let io_vec = vec![__wasi_ciovec_t {
            buf: buffer.as_mut_ptr() as usize as u32,
            buf_len: buffer.len() as u32,
        }];
        let mut io_read = 0;

        let err = unsafe {
            socket_recv(
                client_fd,
                NonNull::new_unchecked(io_vec.as_ptr() as *const _ as *mut _),
                io_vec.len() as u32,
                0,
                &mut io_read,
            )
        };

        if err != __WASI_ESUCCESS {
            panic!("`socket_recv` failed with `{}`", err);
        }

        if io_read < (io_vec.len() as u32) {
            panic!(
                "`socket_recv` has read {} buffers, expected to read {}",
                io_read,
                io_vec.len()
            );
        }

        println!(
            "Read: `{:?}`",
            String::from_utf8_lossy(&buffer[..io_read as usize])
        );

        let mut io_written = 0;
        let err = unsafe {
            socket_send(
                client_fd,
                NonNull::new_unchecked(io_vec.as_ptr() as *const _ as *mut _),
                io_vec.len() as u32,
                0,
                &mut io_written,
            )
        };

        if err != __WASI_ESUCCESS {
            panic!("`socket_send` failed with `{}`", err);
        }

        if io_written < (io_vec.len() as u32) {
            panic!(
                "`socket_send` has written {} buffers, expected to write {}",
                io_written,
                io_vec.len()
            );
        }

        unsafe {
            socket_shutdown(client_fd, SHUT_RDWR);
        }
    }
}
