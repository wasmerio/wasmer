//! Example of (unsafely) using the raw WASIO API.
//! 
//! This example is based upon the `raw_delay` example, with cancellation added.

use wasio::sys::*;
use wasio::types::*;

/// A global registry of tasks for cancellation.
/// 
/// Since this is a quick example I'm using a dirty `static mut` here. High-level
/// libraries should track cancellation tokens properly.
static mut DELAY_TASKS: Vec<CancellationToken> = Vec::new();

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
    const MS: u64 = 1000000;
    const N: u64 = 10;

    println!("Initial task called. Scheduling delayed tasks.");

    for i in 0..N {
        let delay_ns = (1000 + i * 100) * MS; 
        let mut ct = CancellationToken(0);

        // Schedule a callback with `i` as the argument, after `delay_ns` nanoseconds.
        let err = unsafe {
            delay(
                delay_ns,
                make_user_context(delay_callback, i as usize),
                &mut ct
            )
        };
        if err != 0 {
            panic!("initial_task: delay {} failed: {}", i, err);
        }
        unsafe {
            DELAY_TASKS.push(ct);
        }
    }

    println!("Scheduled {} delayed tasks.", N);
}

fn delay_callback(i: usize, err: __wasi_errno_t) {
    println!("Delay {} done with error code {}.", i, err);

    unsafe {
        let tokens = std::mem::replace(&mut DELAY_TASKS, Vec::new());
        let num_cancellations = tokens.len();
        for ct in tokens {
            cancel(ct);
        }
        println!("Cancelled {} task(s).", num_cancellations);
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