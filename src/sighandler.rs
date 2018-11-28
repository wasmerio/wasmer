//! We install signal handlers to handle WebAssembly traps within
//! our Rust code. Otherwise we will have errors that stop the Rust process
//! such as `process didn't exit successfully: ... (signal: 8, SIGFPE: erroneous arithmetic operation)`
//!
//! Please read more about this here: https://github.com/CraneStation/wasmtime/issues/15
//! This code is inspired by: https://github.com/pepyakin/wasmtime/commit/625a2b6c0815b21996e111da51b9664feb174622
use super::recovery;
use nix::libc::{c_void, siginfo_t};
use nix::sys::signal::{
    sigaction, SaFlags, SigAction, SigHandler, SigSet, SIGBUS, SIGFPE, SIGILL, SIGSEGV,
};

pub unsafe fn install_sighandler() {
    let sa = SigAction::new(
        SigHandler::SigAction(signal_trap_handler),
        SaFlags::SA_ONSTACK,
        SigSet::empty(),
    );
    sigaction(SIGFPE, &sa).unwrap();
    sigaction(SIGILL, &sa).unwrap();
    sigaction(SIGSEGV, &sa).unwrap();
    sigaction(SIGBUS, &sa).unwrap();
}

extern "C" fn signal_trap_handler(
    signum: ::nix::libc::c_int,
    siginfo: *mut siginfo_t,
    _ucontext: *mut c_void,
) {
    unsafe {
        recovery::do_unwind(signum, siginfo);
    }
}
