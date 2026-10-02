//! Helpers that are safe to call from inside a signal handler.
//!
//! Very little is: POSIX defines a short list of async-signal-safe functions,
//! and anything else may deadlock or corrupt state if it happens to interrupt
//! the same facility it needs. In particular a Rust panic is not usable here —
//! unwinding out of a signal handler is undefined behaviour, and formatting,
//! allocation and the panic hook are all off-limits regardless.

/// Writes `message` to stderr and aborts the process.
///
/// This stands in for the panic that reporting a broken invariant would
/// otherwise be. `write(2)` and `abort(3)` are both on POSIX's
/// async-signal-safe list, unlike `eprintln!`, which takes a lock, and
/// `exit(3)`, which runs atexit handlers and flushes stdio.
///
/// Aborting rather than exiting leaves a core dump behind, which is what you
/// want for a state that was believed unreachable.
pub(crate) fn die_in_signal_handler(message: &str) -> ! {
    let mut remaining = message.as_bytes();
    // Safety: `write` and `abort` are async-signal-safe.
    unsafe {
        while !remaining.is_empty() {
            let written = libc::write(
                libc::STDERR_FILENO,
                remaining.as_ptr() as *const libc::c_void,
                remaining.len(),
            );
            // Best effort: if stderr can't take the message, there is nothing
            // useful left to do but abort, and retrying could loop forever.
            if written <= 0 {
                break;
            }
            remaining = &remaining[written as usize..];
        }

        libc::abort();
    }
}
