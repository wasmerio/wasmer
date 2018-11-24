//! When a WebAssembly module triggers any traps, we perform recovery here.
//!
//! This module uses TLS (thread-local storage) to track recovery information. Since the four signals we're handling
//! are very special, the async signal unsafety of Rust's TLS implementation generally does not affect the correctness here
//! unless you have memory unsafety elsewhere in your code.

use std::cell::UnsafeCell;

extern "C" {
    fn setjmp(env: *mut ::nix::libc::c_void) -> ::nix::libc::c_int;
    fn longjmp(env: *mut ::nix::libc::c_void, val: ::nix::libc::c_int) -> !;
}

const SETJMP_BUFFER_LEN: usize = 27;

thread_local! {
    static SETJMP_BUFFER: UnsafeCell<[::nix::libc::c_int; SETJMP_BUFFER_LEN]> = UnsafeCell::new([0; SETJMP_BUFFER_LEN]);
}

/// Calls a WebAssembly function with longjmp receiver installed. If a non-WebAssembly function is passed in,
/// the behavior of protected_call is undefined.
pub unsafe fn protected_call<T, R>(f: fn(T) -> R, p: T) -> Result<R, i32> {
    let jmp_buf = SETJMP_BUFFER.with(|buf| buf.get());
    let prev_jmp_buf = *jmp_buf;

    let signum = setjmp(jmp_buf as *mut ::nix::libc::c_void);
    if signum != 0 {
        *jmp_buf = prev_jmp_buf;
        Err(signum)
    } else {
        let ret = f(p); // TODO: Switch stack?
        *jmp_buf = prev_jmp_buf;
        Ok(ret)
    }
}

/// Unwinds to last protected_call.
pub unsafe fn do_unwind(signum: i32) -> ! {
    // Since do_unwind is only expected to get called from WebAssembly code which doesn't hold any host resources (locks etc.)
    // itself, accessing TLS here is safe. In case any other code calls this, it often indicates a memory safety bug and you should
    // temporarily disable the signal handlers to debug it.

    let jmp_buf = SETJMP_BUFFER.with(|buf| buf.get());
    if *jmp_buf == [0; SETJMP_BUFFER_LEN] {
        ::std::process::abort();
    }

    longjmp(jmp_buf as *mut ::nix::libc::c_void, signum)
}
