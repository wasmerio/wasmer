//! Installing signal handlers allows us to handle traps and out-of-bounds memory
//! accesses that occur when runniing WebAssembly.
//!
//! This code is inspired by: https://github.com/pepyakin/wasmtime/commit/625a2b6c0815b21996e111da51b9664feb174622
//!
//! When a WebAssembly module triggers any traps, we perform recovery here.
//!
//! This module uses TLS (thread-local storage) to track recovery information. Since the four signals we're handling
//! are very special, the async signal unsafety of Rust's TLS implementation generally does not affect the correctness here
//! unless you have memory unsafety elsewhere in your code.
//!
use libc::{c_int, c_void, siginfo_t};
use nix::sys::signal::{
    sigaction, SaFlags, SigAction, SigHandler, SigSet, Signal, SIGBUS, SIGFPE, SIGILL, SIGSEGV,
    SIGTRAP,
};
use std::any::Any;
use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::HashMap;
use std::ptr;
use std::sync::Arc;
use std::sync::Once;
use wasmer_runtime_core::codegen::BkptInfo;
use wasmer_runtime_core::state::x64::{read_stack, X64Register, GPR};
use wasmer_runtime_core::typed_func::WasmTrapInfo;
use wasmer_runtime_core::vm;

extern "C" fn signal_trap_handler(
    signum: ::nix::libc::c_int,
    siginfo: *mut siginfo_t,
    ucontext: *mut c_void,
) {
    unsafe {
        let fault = get_fault_info(siginfo as _, ucontext);

        match Signal::from_c_int(signum) {
            Ok(SIGTRAP) => {
                let bkpt_map = BKPT_MAP.with(|x| x.borrow().last().map(|x| x.clone()));
                if let Some(bkpt_map) = bkpt_map {
                    if let Some(ref x) = bkpt_map.get(&(fault.ip as usize)) {
                        (x)(BkptInfo { throw: throw });
                        return;
                    }
                }
            }
            _ => {}
        }

        // TODO: make this safer
        let ctx = &*(fault.known_registers[X64Register::GPR(GPR::R15).to_index().0].unwrap()
            as *mut vm::Ctx);
        let rsp = fault.known_registers[X64Register::GPR(GPR::RSP).to_index().0].unwrap();

        let msm = (*ctx.module)
            .runnable_module
            .get_module_state_map()
            .unwrap();
        let code_base = (*ctx.module).runnable_module.get_code().unwrap().as_ptr() as usize;
        let image = self::read_stack(
            &msm,
            code_base,
            rsp as usize as *const u64,
            fault.known_registers,
            Some(fault.ip as usize as u64),
        );

        use colored::*;
        eprintln!(
            "\n{}",
            "Wasmer encountered an error while running your WebAssembly program."
                .bold()
                .red()
        );
        image.print_backtrace_if_needed();
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
    sigaction(SIGTRAP, &sa).unwrap();
}

const SETJMP_BUFFER_LEN: usize = 27;
pub static SIGHANDLER_INIT: Once = Once::new();

thread_local! {
    pub static SETJMP_BUFFER: UnsafeCell<[c_int; SETJMP_BUFFER_LEN]> = UnsafeCell::new([0; SETJMP_BUFFER_LEN]);
    pub static CAUGHT_FAULTS: Cell<Option<FaultInfo>> = Cell::new(None);
    pub static CURRENT_EXECUTABLE_BUFFER: Cell<*const c_void> = Cell::new(ptr::null());
    pub static TRAP_EARLY_DATA: Cell<Option<Box<dyn Any>>> = Cell::new(None);
    pub static BKPT_MAP: RefCell<Vec<Arc<HashMap<usize, Box<Fn(BkptInfo) + Send + Sync + 'static>>>>> = RefCell::new(Vec::new());
}

pub unsafe fn trigger_trap() -> ! {
    let jmp_buf = SETJMP_BUFFER.with(|buf| buf.get());

    longjmp(jmp_buf as *mut c_void, 0)
}

pub enum CallProtError {
    Trap(WasmTrapInfo),
    Error(Box<dyn Any>),
}

pub fn call_protected<T>(f: impl FnOnce() -> T) -> Result<T, CallProtError> {
    unsafe {
        let jmp_buf = SETJMP_BUFFER.with(|buf| buf.get());
        let prev_jmp_buf = *jmp_buf;

        SIGHANDLER_INIT.call_once(|| {
            install_sighandler();
        });

        let signum = setjmp(jmp_buf as *mut _);
        if signum != 0 {
            *jmp_buf = prev_jmp_buf;

            if let Some(data) = TRAP_EARLY_DATA.with(|cell| cell.replace(None)) {
                Err(CallProtError::Error(data))
            } else {
                // let (faulting_addr, _inst_ptr) = CAUGHT_ADDRESSES.with(|cell| cell.get());

                // let signal = match Signal::from_c_int(signum) {
                //     Ok(SIGFPE) => "floating-point exception",
                //     Ok(SIGILL) => "illegal instruction",
                //     Ok(SIGSEGV) => "segmentation violation",
                //     Ok(SIGBUS) => "bus error",
                //     Err(_) => "error while getting the Signal",
                //     _ => "unknown trapped signal",
                // };
                // // When the trap-handler is fully implemented, this will return more information.
                // Err(RuntimeError::Trap {
                //     msg: format!("unknown trap at {:p} - {}", faulting_addr, signal).into(),
                // }
                // .into())
                Err(CallProtError::Trap(WasmTrapInfo::Unknown))
            }
        } else {
            let ret = f(); // TODO: Switch stack?
            *jmp_buf = prev_jmp_buf;
            Ok(ret)
        }
    }
}

pub unsafe fn throw(payload: Box<dyn Any>) -> ! {
    let jmp_buf = SETJMP_BUFFER.with(|buf| buf.get());
    if *jmp_buf == [0; SETJMP_BUFFER_LEN] {
        ::std::process::abort();
    }
    TRAP_EARLY_DATA.with(|cell| cell.replace(Some(payload)));
    longjmp(jmp_buf as *mut ::nix::libc::c_void, 0xffff);
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

    CAUGHT_FAULTS.with(|cell| cell.set(Some(get_fault_info(siginfo, ucontext))));

    longjmp(jmp_buf as *mut ::nix::libc::c_void, signum)
}

pub struct FaultInfo {
    faulting_addr: *const c_void,
    ip: *const c_void,
    known_registers: [Option<u64>; 24],
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
unsafe fn get_fault_info(siginfo: *const c_void, ucontext: *const c_void) -> FaultInfo {
    use libc::{
        ucontext_t, R10, R11, R12, R13, R14, R15, R8, R9, RAX, RBP, RBX, RCX, RDI, RDX, RIP, RSI,
        RSP,
    };

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
    let gregs = &(*ucontext).uc_mcontext.gregs;

    let mut known_registers: [Option<u64>; 24] = [None; 24];

    known_registers[X64Register::GPR(GPR::R15).to_index().0] = Some(gregs[R15 as usize] as _);
    known_registers[X64Register::GPR(GPR::R14).to_index().0] = Some(gregs[R14 as usize] as _);
    known_registers[X64Register::GPR(GPR::R13).to_index().0] = Some(gregs[R13 as usize] as _);
    known_registers[X64Register::GPR(GPR::R12).to_index().0] = Some(gregs[R12 as usize] as _);
    known_registers[X64Register::GPR(GPR::R11).to_index().0] = Some(gregs[R11 as usize] as _);
    known_registers[X64Register::GPR(GPR::R10).to_index().0] = Some(gregs[R10 as usize] as _);
    known_registers[X64Register::GPR(GPR::R9).to_index().0] = Some(gregs[R9 as usize] as _);
    known_registers[X64Register::GPR(GPR::R8).to_index().0] = Some(gregs[R8 as usize] as _);
    known_registers[X64Register::GPR(GPR::RSI).to_index().0] = Some(gregs[RSI as usize] as _);
    known_registers[X64Register::GPR(GPR::RDI).to_index().0] = Some(gregs[RDI as usize] as _);
    known_registers[X64Register::GPR(GPR::RDX).to_index().0] = Some(gregs[RDX as usize] as _);
    known_registers[X64Register::GPR(GPR::RCX).to_index().0] = Some(gregs[RCX as usize] as _);
    known_registers[X64Register::GPR(GPR::RBX).to_index().0] = Some(gregs[RBX as usize] as _);
    known_registers[X64Register::GPR(GPR::RAX).to_index().0] = Some(gregs[RAX as usize] as _);

    known_registers[X64Register::GPR(GPR::RBP).to_index().0] = Some(gregs[RBP as usize] as _);
    known_registers[X64Register::GPR(GPR::RSP).to_index().0] = Some(gregs[RSP as usize] as _);

    // TODO: XMM registers

    FaultInfo {
        faulting_addr: si_addr as usize as _,
        ip: gregs[RIP as usize] as _,
        known_registers,
    }
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
unsafe fn get_fault_info(siginfo: *const c_void, ucontext: *const c_void) -> FaultInfo {
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
    let ss = &(*(*ucontext).uc_mcontext).ss;

    let mut known_registers: [Option<u64>; 24] = [None; 24];

    known_registers[X64Register::GPR(GPR::R15).to_index().0] = Some(ss.r15);
    known_registers[X64Register::GPR(GPR::R14).to_index().0] = Some(ss.r14);
    known_registers[X64Register::GPR(GPR::R13).to_index().0] = Some(ss.r13);
    known_registers[X64Register::GPR(GPR::R12).to_index().0] = Some(ss.r12);
    known_registers[X64Register::GPR(GPR::R11).to_index().0] = Some(ss.r11);
    known_registers[X64Register::GPR(GPR::R10).to_index().0] = Some(ss.r10);
    known_registers[X64Register::GPR(GPR::R9).to_index().0] = Some(ss.r9);
    known_registers[X64Register::GPR(GPR::R8).to_index().0] = Some(ss.r8);
    known_registers[X64Register::GPR(GPR::RSI).to_index().0] = Some(ss.rsi);
    known_registers[X64Register::GPR(GPR::RDI).to_index().0] = Some(ss.rdi);
    known_registers[X64Register::GPR(GPR::RDX).to_index().0] = Some(ss.rdx);
    known_registers[X64Register::GPR(GPR::RCX).to_index().0] = Some(ss.rcx);
    known_registers[X64Register::GPR(GPR::RBX).to_index().0] = Some(ss.rbx);
    known_registers[X64Register::GPR(GPR::RAX).to_index().0] = Some(ss.rax);

    known_registers[X64Register::GPR(GPR::RBP).to_index().0] = Some(ss.rbp);
    known_registers[X64Register::GPR(GPR::RSP).to_index().0] = Some(ss.rsp);

    // TODO: XMM registers

    FaultInfo {
        faulting_addr: si_addr,
        ip: ss.rip as _,
        known_registers,
    }
}
