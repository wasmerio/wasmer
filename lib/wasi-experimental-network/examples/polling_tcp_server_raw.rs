use wasmer_wasi_experimental_network::{abi::*, types::*};

fn main() {
    println!("Creating the socket");

    let server = {
        let mut server: __wasi_fd_t = 0;
        let err = unsafe { socket_create(AF_INET, SOCK_STREAM, DEFAULT_PROTOCOL, &mut server) };

        if err != __WASI_ESUCCESS {
            panic!("`socket_create` failed with `{}`", err);
        }

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

        let err = unsafe { socket_bind(server, &address) };

        if err != __WASI_ESUCCESS {
            panic!("`socket_bind` failed with `{}`", err);
        }
    }

    println!("Listening");

    {
        let err = unsafe { socket_listen(server, 3) };

        if err != __WASI_ESUCCESS {
            panic!("`socket_listen` failed with `{}`", err);
        }
    }

    println!("Non-blocking mode");

    {
        let err = unsafe { socket_set_nonblocking(server, true) };

        if err != __WASI_ESUCCESS {
            panic!("`socket_set_nonblocking` failed with `{}`", err);
        }
    }

    println!("Creating the poller");

    let poll = {
        let mut poll: __wasi_poll_t = 0;
        let err = unsafe { poller_create(&mut poll) };

        if err != __WASI_ESUCCESS {
            panic!("`poller_create` failed with `{}`", err);
        }

        poll
    };

    let token: __wasi_poll_token_t = 7;

    println!("Registering the server to the poller");

    {
        let err = unsafe {
            poller_add(
                poll,
                server,
                __wasi_poll_event_t {
                    token: token,
                    readable: true,
                    writable: false,
                },
            )
        };

        if err != __WASI_ESUCCESS {
            panic!("`poller_add` failed with `{}`", err);
        }
    }

    println!("Looping");

    loop {
        let mut events: Vec<__wasi_poll_event_t> = Vec::with_capacity(128);
        let mut number_of_events = 0;
        let err = unsafe {
            poller_wait(
                poll,
                events.as_mut_ptr(),
                events.capacity() as u32,
                &mut number_of_events,
            )
        };

        if err != __WASI_ESUCCESS {
            panic!("`poller_wait` failed with `{}`", err);
        }

        unsafe { events.set_len(number_of_events as usize) };

        println!("number of events {}", number_of_events);
        println!("number of events {}", events.len());
        dbg!(&events);
    }
}
