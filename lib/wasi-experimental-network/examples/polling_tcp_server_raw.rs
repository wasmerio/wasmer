use std::collections::HashMap;
use std::mem::MaybeUninit;
use wasmer_wasi_experimental_network::{abi::*, types::*};

macro_rules! syscall {
    ( $function_name:ident ( $( $arguments:expr ),* $(,)* ) ) => {
        let err  = unsafe { $function_name( $( $arguments ),* ) };

        if err != __WASI_ESUCCESS {
            panic!(concat!("`", stringify!($function_name), "` failed with `{}`"), err);
        }
    }
}

fn main() {
    println!("Creating the socket");

    let server = {
        let mut server: __wasi_fd_t = 0;
        syscall!(socket_create(
            AF_INET,
            SOCK_STREAM,
            DEFAULT_PROTOCOL,
            &mut server
        ));

        server
    };

    println!("Binding the socket");

    {
        let address = __wasi_socket_address_t {
            v4: __wasi_socket_address_in_t {
                family: AF_INET,
                address: [0; 4],
                port: 9000u16.to_be(),
            },
        };

        syscall!(socket_bind(server, &address));
    }

    println!("Listening");

    {
        syscall!(socket_listen(server, 3));
    }

    println!("Non-blocking mode");

    {
        syscall!(socket_set_nonblocking(server, true));
    }

    println!("Creating the poller");

    let poll = {
        let mut poll: __wasi_poll_t = 0;
        syscall!(poller_create(&mut poll));

        poll
    };

    const SERVER_TOKEN: __wasi_poll_token_t = 1;

    println!("Registering the server to the poller");

    {
        syscall!(poller_add(
            poll,
            server,
            __wasi_poll_event_t {
                token: SERVER_TOKEN,
                readable: true,
                writable: false,
            },
        ));
    }

    println!("Looping");

    let mut events: Vec<__wasi_poll_event_t> = Vec::with_capacity(128);
    let mut unique_token: __wasi_poll_token_t = SERVER_TOKEN + 1;
    let mut clients: HashMap<__wasi_poll_token_t, __wasi_fd_t> = HashMap::new();

    loop {
        events.clear();
        let mut number_of_events = 0;

        println!("Waiting for new events");

        syscall!(poller_wait(
            poll,
            events.as_mut_ptr(),
            events.capacity() as u32,
            &mut number_of_events,
        ));

        unsafe { events.set_len(number_of_events as usize) };

        println!("Received {} new events", number_of_events);

        for event in events.iter() {
            dbg!(&event);

            match event.token {
                SERVER_TOKEN => {
                    println!("Accepting new connections");

                    loop {
                        let client = {
                            let mut client: __wasi_fd_t = 0;
                            let mut client_address =
                                MaybeUninit::<__wasi_socket_address_t>::uninit();
                            let err = unsafe {
                                socket_accept(server, client_address.as_mut_ptr(), &mut client)
                            };

                            match err {
                                __WASI_ESUCCESS => {
                                    let client_address = unsafe { client_address.assume_init() };
                                    println!("Accepted connection from: `{:?}`", &client_address);

                                    client
                                }

                                // If we get a `WouldBlock` error, we know
                                // our listener has no more incoming
                                // connections queued, so we can return to
                                // polling and wait for some more.
                                __WASI_EAGAIN => break,

                                // If it was any other kind of error,
                                // something went wrong and we terminate
                                // with an error.
                                _ => panic!("`socket_accept` failed with `{}`", err),
                            }
                        };

                        println!("Registering the new connection (only writable events)");

                        {
                            let client_token = next(&mut unique_token);

                            syscall!(poller_add(
                                poll,
                                client,
                                __wasi_poll_event_t {
                                    token: client_token,
                                    readable: false,
                                    writable: true,
                                },
                            ));

                            clients.insert(client_token, client);
                        }
                    }

                    println!("Re-registering the server");

                    {
                        syscall!(poller_modify(
                            poll,
                            server,
                            __wasi_poll_event_t {
                                token: SERVER_TOKEN,
                                readable: true,
                                writable: true,
                            },
                        ));
                    }
                }

                client_token => {
                    // Maybe received an event for a TCP connection.
                    if let Some(client) = clients.get(&client_token) {
                        let client = *client;

                        let close_connection = if event.writable == true {
                            println!("Sending “Welcome!” to the client");

                            let string = "Welcome!\n";
                            let buffer = string.as_bytes();
                            let io_vec = vec![__wasi_ciovec_t {
                                buf: buffer.as_ptr() as usize as u32,
                                buf_len: buffer.len() as u32,
                            }];

                            let mut io_written = 0;
                            syscall!(socket_send(
                                client,
                                io_vec.as_ptr(),
                                io_vec.len() as u32,
                                0,
                                &mut io_written,
                            ));

                            if io_written < (io_vec.len() as u32) {
                                panic!(
                                    "`socket_send` has written {} buffers, expected to write {}",
                                    io_written,
                                    io_vec.len()
                                );
                            }

                            println!("Re-registering the new connection (only readable events)");

                            // After we've written something, we
                            // will re-register the connection to
                            // only respond to readable events.
                            syscall!(poller_modify(
                                poll,
                                client,
                                __wasi_poll_event_t {
                                    token: client_token,
                                    readable: true,
                                    writable: false,
                                }
                            ));

                            false
                        } else if event.readable == true {
                            println!("Receiving the message from the client");

                            let mut buffer: Vec<u8> = vec![0; 128];
                            let mut io_vec = vec![__wasi_ciovec_t {
                                buf: buffer.as_mut_ptr() as usize as u32,
                                buf_len: buffer.len() as u32,
                            }];
                            let mut io_read = 0;

                            syscall!(socket_recv(
                                client,
                                io_vec.as_mut_ptr(),
                                io_vec.len() as u32,
                                0,
                                &mut io_read,
                            ));

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

                            true
                        } else {
                            true
                        };

                        if close_connection {
                            println!("Closing the client {}", client);

                            clients.remove(&client_token);
                            syscall!(socket_close(client));
                        }
                    } else {
                        // Sporadic events happen, we can safely ignore them.
                    }
                }
            }
        }
    }
}

fn next(token: &mut __wasi_poll_token_t) -> __wasi_poll_token_t {
    let next = *token;
    *token += 1;

    next
}
