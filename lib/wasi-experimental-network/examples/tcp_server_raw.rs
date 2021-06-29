use wasmer_wasi_experimental_network::{abi::*, types::*};

fn main() {
    println!("Creating the socket");

    let fd = {
        let mut fd: __wasi_fd_t = 0;
        let err = unsafe { socket_create(AF_INET, SOCK_STREAM, DEFAULT_PROTOCOL, &mut fd) };

        if err != __WASI_ESUCCESS {
            panic!("`socket_create` failed with `{}`", err);
        }

        fd
    };

    println!("Binding the socket");

    let address = __wasi_socket_address_t {
        v4: __wasi_socket_address_in_t {
            family: AF_INET,
            address: [0; 4],
            port: 9000u16.to_be(),
        },
    };

    let err = unsafe { socket_bind(fd, &address) };

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
        let mut client_address = __wasi_socket_address_t {
            v4: __wasi_socket_address_in_t::default(),
        };
        let err = unsafe { socket_accept(fd, &mut client_address, &mut client_fd) };

        println!("Remote client IP: `{:?}`", &client_address);

        if err != __WASI_ESUCCESS {
            panic!("`socket_accept` failed with `{}`", err);
        }

        let mut buffer: Vec<u8> = vec![0; 128];
        let mut io_vec = vec![__wasi_ciovec_t {
            buf: buffer.as_mut_ptr() as usize as u32,
            buf_len: buffer.len() as u32,
        }];
        let mut io_read = 0;

        let err = unsafe {
            socket_recv(
                client_fd,
                io_vec.as_mut_ptr(),
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

        /*
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
        */
    }
}
