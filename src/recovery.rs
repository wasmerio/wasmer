//! When a WebAssembly module triggers any traps, we perform recovery here.
//!
//! This module uses TLS (thread-local storage) to track recovery information. Since the four signals we're handling
//! are very special, the async signal unsafety of Rust's TLS implementation generally does not affect the correctness here
//! unless you have memory unsafety elsewhere in your code.

use nix::libc::siginfo_t;
use std::cell::{Cell, UnsafeCell};
use std::sync::Once;

extern "C" {
    pub fn setjmp(env: *mut ::nix::libc::c_void) -> ::nix::libc::c_int;
    fn longjmp(env: *mut ::nix::libc::c_void, val: ::nix::libc::c_int) -> !;
}

const SETJMP_BUFFER_LEN: usize = 27;
pub static SIGHANDLER_INIT: Once = Once::new();

thread_local! {
    pub static SETJMP_BUFFER: UnsafeCell<[::nix::libc::c_int; SETJMP_BUFFER_LEN]> = UnsafeCell::new([0; SETJMP_BUFFER_LEN]);
    pub static CAUGHT_ADDRESS: Cell<usize> = Cell::new(0);
}

// We need a macro since the arguments we will provide to the funciton
// (and the return value) are not fixed to just one case: f(x) -> y
// but multiple: f(x) -> y, f(a,b) -> c, ...
// And right now it's impossible to handle with Rust function type system

/// Calls a WebAssembly function with longjmp receiver installed. If a non-WebAssembly function is passed in,
/// the behavior of call_protected is undefined.
#[macro_export]
macro_rules! call_protected {
    ($x:expr) => {
        unsafe {
            use crate::recovery::{setjmp, CAUGHT_ADDRESS, SETJMP_BUFFER, SIGHANDLER_INIT};
            use crate::sighandler::install_sighandler;
            use crate::webassembly::ErrorKind;

            use crate::nix::sys::signal::{Signal, SIGBUS, SIGFPE, SIGILL, SIGSEGV};

            let jmp_buf = SETJMP_BUFFER.with(|buf| buf.get());
            let prev_jmp_buf = *jmp_buf;

            SIGHANDLER_INIT.call_once(|| {
                install_sighandler();
            });

            let signum = setjmp(jmp_buf as *mut ::nix::libc::c_void);
            if signum != 0 {
                *jmp_buf = prev_jmp_buf;
                let addr = CAUGHT_ADDRESS.with(|cell| cell.get());

                let signal = match Signal::from_c_int(signum) {
                    Ok(SIGFPE) => "floating-point exception",
                    Ok(SIGILL) => "illegal instruction",
                    Ok(SIGSEGV) => "segmentation violation",
                    Ok(SIGBUS) => "bus error",
                    Err(_) => "error while getting the Signal",
                    _ => "unkown trapped signal",
                };
                Err(ErrorKind::RuntimeError(format!(
                    "trap at {:#x} - {}",
                    addr, signal
                )))
            } else {
                let ret = $x; // TODO: Switch stack?
                *jmp_buf = prev_jmp_buf;
                Ok(ret)
            }
        }
    };
}

/// Unwinds to last protected_call.
pub unsafe fn do_unwind(signum: i32, siginfo: *mut siginfo_t) -> ! {
    // Since do_unwind is only expected to get called from WebAssembly code which doesn't hold any host resources (locks etc.)
    // itself, accessing TLS here is safe. In case any other code calls this, it often indicates a memory safety bug and you should
    // temporarily disable the signal handlers to debug it.

    let jmp_buf = SETJMP_BUFFER.with(|buf| buf.get());
    if *jmp_buf == [0; SETJMP_BUFFER_LEN] {
        ::std::process::abort();
    }
    // We only target macos at the moment as other ones might not have si_addr field
    #[cfg(target_os="macos")]
    CAUGHT_ADDRESS.with(|cell| cell.set((*siginfo).si_addr as _));

    longjmp(jmp_buf as *mut ::nix::libc::c_void, signum)
}
