//! Example of (unsafely) using the raw WASIO API.
//! 
//! A simple TCP client that connects to the echo server and requests its response.

// Required for `File::from_raw_fd`.
#![feature(wasi_ext)]

use wasio::sys::*;
use wasio::types::*;

fn main() {
    unsafe {
        let mut ct = CancellationToken(0);

        // Schedule the initial task onto the event loop.
        let err = delay(
            0, // 0 nanoseconds - complete immediately.
            make_user_context(initial_task, 0),
            &mut ct
        );

        // Explicitly check the error here, just to be quick.
        if err != 0 {
            panic!("initial delay() error: {}", err);
        }

        // Run the event loop.
        loop {
            let mut err = 0;
            let mut uc: UserContext = UserContext(0);

            // wait() blocks until a event arrives.
            let local_err = wait(&mut err, &mut uc);

            // If the pointers passed to `wait()` are always valid, this should never happen.
            // This check is just for consistency.
            if local_err != 0 {
                panic!("wait() error: {}", local_err);
            }

            // Parse the (callback, callback_data) pair.
            let (callback, callback_data) = parse_user_context(uc);

            // Call the callback.
            callback(callback_data, err);
        }
    }
}

fn initial_task(_: usize, _: __wasi_errno_t) {
    unsafe {
        // Create the socket.
        let mut fd: __wasi_fd_t = 0;
        let err = unsafe { socket_create(&mut fd, AF_INET, SOCK_STREAM, 0) };
        if err != 0 {
            panic!("socket_create failed: {}", err);
        }
        println!("Listener fd: {}", fd);

        // Connect this socket to server.
        let listen_addr = SockaddrIn {
            sin_family: AF_INET as _,
            sin_port: 9000u16.to_be(),
            sin_addr: [127, 0, 0, 1],
            sin_zero: [0; 8]
        };
        let mut ct = CancellationToken(0);
        let err = socket_connect(
            fd,
            &listen_addr as *const _ as *const u8,
            std::mem::size_of::<SockaddrIn>() as u32,
            make_user_context(on_connected, Box::into_raw(Box::new(RwContinuation {
                buffer: vec![0; 2048],
                conn: fd,
                len_buffer: 0,
                count: 0,
                num_bytes: 0,
            })) as usize),
            &mut ct
        );
        if err != 0 {
            panic!("socket_connect failed: {}", err);
        }
    }
}

fn on_connected(continuation: usize, err: __wasi_errno_t) {
    unsafe {
        let mut continuation = Box::from_raw(continuation as *mut RwContinuation);
        if err != 0 {
            panic!("Connect failed: {}", err);
        }
        println!("Connected.");

        // Send out the first message.
        continuation.buffer[0] = b'@';
        continuation.len_buffer = 1;
        write_data(Box::into_raw(continuation) as usize, 0);
    }
}

struct RwContinuation {
    buffer: Vec<u8>,
    conn: __wasi_fd_t,
    len_buffer: u32,
    count: u64,
    num_bytes: u64,
}

fn read_data(continuation: usize, err: __wasi_errno_t) {
    unsafe {
        let mut continuation = Box::from_raw(continuation as *mut RwContinuation);
        if err != 0 {
            println!("error before read_data: {}", err);
            return;
        }

        // Read from the connection.
        let iov = __wasi_ciovec_t {
            buf: continuation.buffer.as_mut_ptr(),
            buf_len: continuation.buffer.len() as u32,
        };
        let mut ct = CancellationToken(0);
        let mut len_buffer = &mut continuation.len_buffer as *mut u32;
        let err = socket_recv(
            continuation.conn,
            &iov,
            1,
            0,
            len_buffer,
            std::ptr::null_mut(),
            make_user_context(write_data, Box::into_raw(continuation) as usize),
            &mut ct
        );
        if err != 0 {
            panic!("read@read_data failed: {}", err);
        }
    }
}

fn write_data(continuation: usize, err: __wasi_errno_t) {
    unsafe {
        let mut continuation = Box::from_raw(continuation as *mut RwContinuation);
        if err != 0 {
            println!("error before write_data: {}", err);
            return;
        }

        if continuation.len_buffer == 0 {
            println!("EOF on connection {}.", continuation.conn);
            close(continuation.conn);
            return;
        }

        continuation.count += 1;
        continuation.num_bytes += continuation.len_buffer as u64;

        if continuation.count % 10000 == 0 {
            println!("Got message {}. Total bytes received = {}", continuation.count, continuation.num_bytes);
        }

        let iov = __wasi_ciovec_t {
            buf: continuation.buffer.as_ptr() as *mut u8,
            buf_len: continuation.len_buffer,
        };
        let mut ct = CancellationToken(0);
        let mut len_buffer = &mut continuation.len_buffer as *mut u32;
        let err = socket_send(
            continuation.conn,
            &iov,
            1,
            0,
            len_buffer,
            make_user_context(read_data, Box::into_raw(continuation) as usize),
            &mut ct
        );
        if err != 0 {
            panic!("write@write_data failed: {}", err);
        }
    }
}

/// Builds a `UserContext` from a (callback, callback_data) pair.
/// 
/// WebAssembly pointers are 32-bit while a `UserContext` is backed by a 64-bit integer.
/// So we can represent a pair of pointers with one `UserContext`.
fn make_user_context(callback: fn (usize, __wasi_errno_t), callback_data: usize) -> UserContext {
    UserContext((callback as u64) | ((callback_data as u64) << 32))
}

/// The reverse operation of `make_user_context`.
/// 
/// Takes a `UserContext`, and converts it into a (callback, callback_data) pair.
unsafe fn parse_user_context(uc: UserContext) -> (fn (usize, __wasi_errno_t), usize) {
    let callback = uc.0 as u32;
    let callback_data = (uc.0 >> 32) as u32;
    (std::mem::transmute(callback), callback_data as usize)
}

/// Closes a file descriptor.
unsafe fn close(fd: __wasi_fd_t) {
    use std::fs::File;
    use std::os::wasi::prelude::FromRawFd;
    unsafe {
        File::from_raw_fd(fd);
    }
}
