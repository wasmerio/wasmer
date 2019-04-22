//! Installing signal handlers allows us to handle traps and out-of-bounds memory
//! accesses that occur when runniing webassembly.
//!
//! This code is inspired by: https://github.com/pepyakin/wasmtime/commit/625a2b6c0815b21996e111da51b9664feb174622
//!
//! When a WebAssembly module triggers any traps, we perform recovery here.
//!
//! This module uses TLS (thread-local storage) to track recovery information. Since the four signals we're handling
//! are very special, the async signal unsafety of Rust's TLS implementation generally does not affect the correctness here
//! unless you have memory unsafety elsewhere in your code.
//!
use crate::relocation::{TrapCode, TrapData};
use crate::signal::{CallProtError, HandlerData};
use libc::{c_int, c_void, siginfo_t};
use nix::sys::signal::{
    sigaction, SaFlags, SigAction, SigHandler, SigSet, Signal, SIGBUS, SIGFPE, SIGILL, SIGSEGV,
};
use std::cell::{Cell, UnsafeCell};
use std::ptr;
use std::sync::Once;
use wasmer_runtime_core::typed_func::WasmTrapInfo;

extern "C" fn signal_trap_handler(
    signum: ::nix::libc::c_int,
    siginfo: *mut siginfo_t,
    ucontext: *mut c_void,
) {
    unsafe {
        do_unwind(signum, siginfo as _, ucontext);
    }
}

extern "C" {
    pub fn setjmp(env: *mut c_void) -> c_int;
    fn longjmp(env: *mut c_void, val: c_int) -> !;
}

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

const SETJMP_BUFFER_LEN: usize = 27;
pub static SIGHANDLER_INIT: Once = Once::new();

thread_local! {
    pub static SETJMP_BUFFER: UnsafeCell<[c_int; SETJMP_BUFFER_LEN]> = UnsafeCell::new([0; SETJMP_BUFFER_LEN]);
    pub static CAUGHT_ADDRESSES: Cell<(*const c_void, *const c_void)> = Cell::new((ptr::null(), ptr::null()));
    pub static CURRENT_EXECUTABLE_BUFFER: Cell<*const c_void> = Cell::new(ptr::null());
}

pub unsafe fn trigger_trap() -> ! {
    let jmp_buf = SETJMP_BUFFER.with(|buf| buf.get());

    longjmp(jmp_buf as *mut c_void, 0)
}

pub fn call_protected<T>(
    handler_data: &HandlerData,
    f: impl FnOnce() -> T,
) -> Result<T, CallProtError> {
    unsafe {
        let jmp_buf = SETJMP_BUFFER.with(|buf| buf.get());
        let prev_jmp_buf = *jmp_buf;

        SIGHANDLER_INIT.call_once(|| {
            install_sighandler();
        });

        let signum = setjmp(jmp_buf as *mut _);
        if signum != 0 {
            *jmp_buf = prev_jmp_buf;

            if let Some(data) = super::TRAP_EARLY_DATA.with(|cell| cell.replace(None)) {
                Err(CallProtError::Error(data))
            } else {
                let (faulting_addr, inst_ptr) = CAUGHT_ADDRESSES.with(|cell| cell.get());

                if let Some(TrapData {
                    trapcode,
                    srcloc: _,
                }) = handler_data.lookup(inst_ptr)
                {
                    Err(CallProtError::Trap(match Signal::from_c_int(signum) {
                        Ok(SIGILL) => match trapcode {
                            TrapCode::BadSignature => WasmTrapInfo::IncorrectCallIndirectSignature,
                            TrapCode::IndirectCallToNull => WasmTrapInfo::CallIndirectOOB,
                            TrapCode::HeapOutOfBounds => WasmTrapInfo::MemoryOutOfBounds,
                            TrapCode::TableOutOfBounds => WasmTrapInfo::CallIndirectOOB,
                            _ => WasmTrapInfo::Unknown,
                        },
                        Ok(SIGSEGV) | Ok(SIGBUS) => WasmTrapInfo::MemoryOutOfBounds,
                        Ok(SIGFPE) => WasmTrapInfo::IllegalArithmetic,
                        _ => unimplemented!(),
                    }))
                } else {
                    let signal = match Signal::from_c_int(signum) {
                        Ok(SIGFPE) => "floating-point exception",
                        Ok(SIGILL) => "illegal instruction",
                        Ok(SIGSEGV) => "segmentation violation",
                        Ok(SIGBUS) => "bus error",
                        Err(_) => "error while getting the Signal",
                        _ => "unkown trapped signal",
                    };
                    // When the trap-handler is fully implemented, this will return more information.
                    let s = format!("unknown trap at {:p} - {}", faulting_addr, signal);
                    Err(CallProtError::Error(Box::new(s)))
                }
            }
        } else {
            let ret = f(); // TODO: Switch stack?
            *jmp_buf = prev_jmp_buf;
            Ok(ret)
        }
    }
}

/// Unwinds to last protected_call.
pub unsafe fn do_unwind(signum: i32, siginfo: *const c_void, ucontext: *const c_void) -> ! {
    // Since do_unwind is only expected to get called from WebAssembly code which doesn't hold any host resources (locks etc.)
    // itself, accessing TLS here is safe. In case any other code calls this, it often indicates a memory safety bug and you should
    // temporarily disable the signal handlers to debug it.

    let jmp_buf = SETJMP_BUFFER.with(|buf| buf.get());
    if *jmp_buf == [0; SETJMP_BUFFER_LEN] {
        ::std::process::abort();
    }

    CAUGHT_ADDRESSES.with(|cell| cell.set(get_faulting_addr_and_ip(siginfo, ucontext)));

    longjmp(jmp_buf as *mut ::nix::libc::c_void, signum)
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
unsafe fn get_faulting_addr_and_ip(
    siginfo: *const c_void,
    ucontext: *const c_void,
) -> (*const c_void, *const c_void) {
    use libc::{ucontext_t, RIP};

    #[allow(dead_code)]
    #[repr(C)]
    struct siginfo_t {
        si_signo: i32,
        si_errno: i32,
        si_code: i32,
        si_addr: u64,
        // ...
    }

    let siginfo = siginfo as *const siginfo_t;
    let si_addr = (*siginfo).si_addr;

    let ucontext = ucontext as *const ucontext_t;
    let rip = (*ucontext).uc_mcontext.gregs[RIP as usize];

    (si_addr as _, rip as _)
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
unsafe fn get_faulting_addr_and_ip(
    siginfo: *const c_void,
    ucontext: *const c_void,
) -> (*const c_void, *const c_void) {
    #[allow(dead_code)]
    #[repr(C)]
    struct ucontext_t {
        uc_onstack: u32,
        uc_sigmask: u32,
        uc_stack: libc::stack_t,
        uc_link: *const ucontext_t,
        uc_mcsize: u64,
        uc_mcontext: *const mcontext_t,
    }
    #[repr(C)]
    struct exception_state {
        trapno: u16,
        cpu: u16,
        err: u32,
        faultvaddr: u64,
    }
    #[repr(C)]
    struct regs {
        rax: u64,
        rbx: u64,
        rcx: u64,
        rdx: u64,
        rdi: u64,
        rsi: u64,
        rbp: u64,
        rsp: u64,
        r8: u64,
        r9: u64,
        r10: u64,
        r11: u64,
        r12: u64,
        r13: u64,
        r14: u64,
        r15: u64,
        rip: u64,
        rflags: u64,
        cs: u64,
        fs: u64,
        gs: u64,
    }
    #[allow(dead_code)]
    #[repr(C)]
    struct mcontext_t {
        es: exception_state,
        ss: regs,
        // ...
    }

    let siginfo = siginfo as *const siginfo_t;
    let si_addr = (*siginfo).si_addr;

    let ucontext = ucontext as *const ucontext_t;
    let rip = (*(*ucontext).uc_mcontext).ss.rip;

    (si_addr, rip as _)
}

#[cfg(not(any(
    all(target_os = "macos", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64"),
)))]
compile_error!("This crate doesn't yet support compiling on operating systems other than linux and macos and architectures other than x86_64");
