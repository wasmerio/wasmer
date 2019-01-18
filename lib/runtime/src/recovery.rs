//! When a WebAssembly module triggers any traps, we perform recovery here.
//!
//! This module uses TLS (thread-local storage) to track recovery information. Since the four signals we're handling
//! are very special, the async signal unsafety of Rust's TLS implementation generally does not affect the correctness here
//! unless you have memory unsafety elsewhere in your code.

use crate::{
    error::{RuntimeError, RuntimeResult},
    sighandler::install_sighandler,
};
use nix::libc::siginfo_t;
use nix::sys::signal::{Signal, SIGBUS, SIGFPE, SIGILL, SIGSEGV};
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

pub fn call_protected<T>(f: impl FnOnce() -> T) -> RuntimeResult<T> {
    unsafe {
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
            // When the trap-handler is fully implemented, this will return more information.
            Err(RuntimeError::Unknown {
                msg: format!("trap at {:#x} - {}", addr, signal),
            }
            .into())
        } else {
            let ret = f(); // TODO: Switch stack?
            *jmp_buf = prev_jmp_buf;
            Ok(ret)
        }
    }
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
    #[cfg(target_os = "macos")]
    CAUGHT_ADDRESS.with(|cell| cell.set((*siginfo).si_addr as _));

    longjmp(jmp_buf as *mut ::nix::libc::c_void, signum)
}
