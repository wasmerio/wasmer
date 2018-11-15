//! We install signal handlers to handle WebAssembly traps within
//! our Rust code. Otherwise we will have errors that stop the Rust process
//! such as `process didn't exit successfully: ... (signal: 8, SIGFPE: erroneous arithmetic operation)`
//!
//! Please read more about this here: https://github.com/CraneStation/wasmtime/issues/15
//! This code is inspired by: https://github.com/pepyakin/wasmtime/commit/625a2b6c0815b21996e111da51b9664feb174622
use nix::sys::signal::{
    sigaction, SaFlags, SigAction, SigHandler, SigSet, Signal, SIGBUS, SIGFPE, SIGILL, SIGSEGV,
};

static mut SETJMP_BUFFER: [::nix::libc::c_int; 27] = [0; 27];

pub unsafe fn install_sighandler() {
    let sa = SigAction::new(
        SigHandler::Handler(signal_trap_handler),
        SaFlags::empty(),
        SigSet::empty(),
    );
    sigaction(SIGFPE, &sa).unwrap();
    sigaction(SIGILL, &sa).unwrap();
    sigaction(SIGSEGV, &sa).unwrap();
    sigaction(SIGBUS, &sa).unwrap();
    let signum = setjmp((&mut SETJMP_BUFFER[..]).as_mut_ptr() as *mut ::nix::libc::c_void);
    if signum != 0 {
        let signal = Signal::from_c_int(signum).unwrap();
        match signal {
            SIGFPE => panic!("floating-point exception"),
            SIGILL => panic!("illegal instruction"),
            SIGSEGV => panic!("segmentation violation"),
            SIGBUS => panic!("bus error"),
            _ => panic!("signal error: {:?}", signal),
        };
    }
}

extern "C" {
    fn setjmp(env: *mut ::nix::libc::c_void) -> ::nix::libc::c_int;
    fn longjmp(env: *mut ::nix::libc::c_void, val: ::nix::libc::c_int);
}

extern "C" fn signal_trap_handler(signum: ::nix::libc::c_int) {
    unsafe {
        longjmp(
            (&mut SETJMP_BUFFER).as_mut_ptr() as *mut ::nix::libc::c_void,
            signum,
        );
    }
}
