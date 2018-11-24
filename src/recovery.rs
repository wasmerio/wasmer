//! When a WebAssembly module triggers any traps, we perform recovery here.

use std::cell::UnsafeCell;

extern "C" {
    fn setjmp(env: *mut ::nix::libc::c_void) -> ::nix::libc::c_int;
    fn longjmp(env: *mut ::nix::libc::c_void, val: ::nix::libc::c_int) -> !;
}

const SETJMP_BUFFER_LEN: usize = 27;

thread_local! {
    static SETJMP_BUFFER: UnsafeCell<[::nix::libc::c_int; SETJMP_BUFFER_LEN]> = UnsafeCell::new([0; SETJMP_BUFFER_LEN]);
}

/// Calls a function with longjmp receiver installed. The function must be compiled from WebAssembly;
/// Otherwise, the behavior is undefined.
pub unsafe fn protected_call<T, R>(f: fn(T) -> R, p: T) -> Result<R, i32> {
    let jmp_buf = SETJMP_BUFFER.with(|buf| buf.get());
    let prev_jmp_buf = *jmp_buf;

    let signum = setjmp(jmp_buf as *mut ::nix::libc::c_void);
    if signum != 0 {
        *jmp_buf = prev_jmp_buf;
        Err(signum)
    } else {
        let ret = f(p);
        *jmp_buf = prev_jmp_buf;
        Ok(ret)
    }
}

/// Unwinds to last protected_call.
pub unsafe fn do_unwind(signum: i32) -> ! {
    let jmp_buf = SETJMP_BUFFER.with(|buf| buf.get());
    if *jmp_buf == [0; SETJMP_BUFFER_LEN] {
        ::std::process::abort();
    }

    longjmp(jmp_buf as *mut ::nix::libc::c_void, signum)
}
