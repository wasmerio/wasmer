use std::mem::MaybeUninit;
use std::ptr::NonNull;
use wasmer_wasi_experimental_network::{
    abi::*,
    polling::{abi::*, types::*},
    types::*,
};

const SERVER: __wasi_poll_token_t = 0;

fn main() {
    // Create a poll instance.
    let mut polll = MaybeUninit::<__wasi_poll_t>::uninit();
    let err = unsafe { poll_create(polll.as_mut_ptr()) };

    if err != __WASI_ESUCCESS {
        panic!("`poll_create` failed with `{}`", err);
    }

    let polll = unsafe { polll.assume_init() };

    // Create storage for events.
    let mut events = MaybeUninit::<__wasi_poll_events_t>::uninit();
    let err = unsafe { events_create(128, events.as_mut_ptr()) };

    if err != __WASI_ESUCCESS {
        panic!("`events_create` failed with `{}", err);
    }

    let events = unsafe { events.assume_init() };

    // Setup the TCP server socket.
    let server = {
        let mut fd: __wasi_fd_t = 0;
        let err = unsafe { socket_create(&mut fd, AF_INET, SOCK_STREAM, DEFAULT_PROTOCOL) };

        if err != 0 {
            panic!("`socket_create` failed with `{}`", err);
        }

        fd
    };

    let address = SockaddrIn {
        sin_family: AF_INET as _,
        sin_port: 9000u16.to_be(),
        sin_addr: [0; 4],
        sin_zero: [0; 8],
    };

    let err = unsafe {
        socket_bind(
            server,
            NonNull::new_unchecked(&address as *const _ as *mut _),
            address.size_of_self(),
        )
    };

    if err != __WASI_ESUCCESS {
        panic!("`socket_bind` failed with `{}`", err);
    }

    let err = unsafe { socket_listen(server, 128) };

    if err != __WASI_ESUCCESS {
        panic!("`socket_listen` failed with `{}`", err);
    }

    // Register the server with poll we can receive events for it.
    let err = unsafe { poll_register(polll, server, SERVER, READABLE_INTEREST) };

    if err != __WASI_ESUCCESS {
        panic!("`poll_register` failed with `{}`", err);
    }

    println!("Starting the loop");

    let mut unique_token: __wasi_poll_token_t = SERVER + 1;

    // Here we go.
    loop {
        let mut number_of_events = 0;
        let err = unsafe { poll(polll, events, &mut number_of_events) };

        if err != __WASI_ESUCCESS {
            panic!("`poll` failed with `{}`", err);
        }

        println!("Received events: {}", number_of_events);

        for event_nth in 0..number_of_events {
            let mut token = 0;
            let err = unsafe { event_token(events, event_nth, &mut token) };

            if err != __WASI_ESUCCESS {
                panic!("`event_token` failed with `{}`", err);
            }

            match token {
                SERVER => loop {
                    dbg!("on the server");

                    // Received an event for the TCP server socket,
                    // which indicates we can accept a connection.
                    let mut client_fd: __wasi_fd_t = 0;
                    let mut client_address = SockaddrIn::default();
                    let mut client_address_size = client_address.size_of_self();
                    let err = unsafe {
                        socket_accept(
                            server,
                            &mut client_address as *mut _ as *mut u8,
                            &mut client_address_size,
                            &mut client_fd,
                        )
                    };

                    println!("Remote client IP: `{:?}`", &client_address);

                    dbg!(err);

                    match err {
                        __WASI_ESUCCESS => {
                            dbg!("it's OK");
                            ()
                        }
                        __WASI_EAGAIN => {
                            dbg!("would block");
                            // If we get a `WouldBlock` error we know
                            // our listener has no more incoming
                            // connections queued, so we can return to
                            // polling and wait for some more.
                            break;
                        }
                        err => {
                            // If it was any other kind of error,
                            // something went wrong and we terminate
                            // with an error.
                            panic!("`socket_accept` failed with `{}`", err);
                        }
                    }
                },
                token => {
                    dbg!(format!("another token {}", token));
                    ()
                }
            }
        }

        break;
    }

    /*

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

        if err != 0 {
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

        if err != 0 {
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

        if err != 0 {
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
    */
}
