mod raw {
    use std::ffi::c_void;

    extern "C" {
        pub fn run_on_alternative_stack(
            stack_end: *mut u64,
            stack_begin: *mut u64,
            userdata_arg2: *mut u8,
        ) -> u64;
        pub fn setjmp(env: *mut c_void) -> i32;
        pub fn longjmp(env: *mut c_void, val: i32) -> !;
    }
}

use crate::state::x64::{read_stack, X64Register, GPR};
use crate::suspend;
use crate::vm;
use libc::siginfo_t;
use nix::sys::signal::{
    sigaction, SaFlags, SigAction, SigHandler, SigSet, SIGBUS, SIGFPE, SIGILL, SIGINT, SIGSEGV,
    SIGTRAP,
};
use std::any::Any;
use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::process;
use std::sync::Once;

pub(crate) unsafe fn run_on_alternative_stack(stack_end: *mut u64, stack_begin: *mut u64) -> u64 {
    raw::run_on_alternative_stack(stack_end, stack_begin, ::std::ptr::null_mut())
}

const SETJMP_BUFFER_LEN: usize = 27;
type SetJmpBuffer = [i32; SETJMP_BUFFER_LEN];

thread_local! {
    static UNWIND: UnsafeCell<Option<(SetJmpBuffer, Option<Box<Any>>)>> = UnsafeCell::new(None);
}

pub unsafe fn catch_unsafe_unwind<R, F: FnOnce() -> R>(f: F) -> Result<R, Box<Any>> {
    let unwind = UNWIND.with(|x| x.get());
    let old = (*unwind).take();
    *unwind = Some(([0; SETJMP_BUFFER_LEN], None));

    if raw::setjmp(&mut (*unwind).as_mut().unwrap().0 as *mut SetJmpBuffer as *mut _) != 0 {
        // error
        let ret = (*unwind).as_mut().unwrap().1.take().unwrap();
        *unwind = old;
        Err(ret)
    } else {
        let ret = f();
        // implicit control flow to the error case...
        *unwind = old;
        Ok(ret)
    }
}

pub unsafe fn begin_unsafe_unwind(e: Box<Any>) -> ! {
    let unwind = UNWIND.with(|x| x.get());
    let inner = (*unwind)
        .as_mut()
        .expect("not within a catch_unsafe_unwind scope");
    inner.1 = Some(e);
    raw::longjmp(&mut inner.0 as *mut SetJmpBuffer as *mut _, 0xffff);
}

pub fn allocate_and_run<R, F: FnOnce() -> R>(size: usize, f: F) -> R {
    struct Context<F: FnOnce() -> R, R> {
        f: Option<F>,
        ret: Option<R>,
    }

    extern "C" fn invoke<F: FnOnce() -> R, R>(_: u64, _: u64, ctx: &mut Context<F, R>) {
        let f = ctx.f.take().unwrap();
        ctx.ret = Some(f());
    }

    unsafe {
        let mut ctx = Context {
            f: Some(f),
            ret: None,
        };
        assert!(size % 16 == 0);
        assert!(size >= 4096);

        let mut stack: Vec<u64> = vec![0; size / 8];
        let end_offset = stack.len();

        stack[end_offset - 4] = invoke::<F, R> as usize as u64;

        // NOTE: Keep this consistent with `image-loading-*.s`.
        let stack_begin = stack.as_mut_ptr().offset((end_offset - 4 - 6) as isize);
        let stack_end = stack.as_mut_ptr().offset(end_offset as isize);

        raw::run_on_alternative_stack(
            stack_end,
            stack_begin,
            &mut ctx as *mut Context<F, R> as *mut u8,
        );
        ctx.ret.take().unwrap()
    }
}

extern "C" fn signal_trap_handler(
    _signum: ::nix::libc::c_int,
    siginfo: *mut siginfo_t,
    ucontext: *mut c_void,
) {
    unsafe {
        let fault = get_fault_info(siginfo as _, ucontext);

        allocate_and_run(65536, || {
            // TODO: make this safer
            let ctx = &*(fault.known_registers[X64Register::GPR(GPR::R15).to_index().0].unwrap()
                as *mut vm::Ctx);
            let rsp = fault.known_registers[X64Register::GPR(GPR::RSP).to_index().0].unwrap();

            let msm = (*ctx.module)
                .runnable_module
                .get_module_state_map()
                .unwrap();
            let code_base = (*ctx.module).runnable_module.get_code().unwrap().as_ptr() as usize;
            let image = read_stack(
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
        });

        begin_unsafe_unwind(Box::new(()));
    }
}

extern "C" fn sigint_handler(
    _signum: ::nix::libc::c_int,
    _siginfo: *mut siginfo_t,
    _ucontext: *mut c_void,
) {
    if suspend::get_interrupted() {
        eprintln!(
            "Got another SIGINT before interrupt is handled by WebAssembly program, aborting"
        );
        process::abort();
    }
    suspend::set_interrupted(true);
    eprintln!("Notified WebAssembly program to exit");
}

pub fn ensure_sighandler() {
    INSTALL_SIGHANDLER.call_once(|| unsafe {
        install_sighandler();
    });
}

static INSTALL_SIGHANDLER: Once = Once::new();

unsafe fn install_sighandler() {
    let sa_trap = SigAction::new(
        SigHandler::SigAction(signal_trap_handler),
        SaFlags::SA_ONSTACK,
        SigSet::empty(),
    );
    sigaction(SIGFPE, &sa_trap).unwrap();
    sigaction(SIGILL, &sa_trap).unwrap();
    sigaction(SIGSEGV, &sa_trap).unwrap();
    sigaction(SIGBUS, &sa_trap).unwrap();
    sigaction(SIGTRAP, &sa_trap).unwrap();

    let sa_interrupt = SigAction::new(
        SigHandler::SigAction(sigint_handler),
        SaFlags::SA_ONSTACK,
        SigSet::empty(),
    );
    sigaction(SIGINT, &sa_interrupt).unwrap();
}

pub struct FaultInfo {
    pub faulting_addr: *const c_void,
    pub ip: *const c_void,
    pub known_registers: [Option<u64>; 24],
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub unsafe fn get_fault_info(siginfo: *const c_void, ucontext: *const c_void) -> FaultInfo {
    use libc::{
        ucontext_t, REG_R10, REG_R11, REG_R12, REG_R13, REG_R14, REG_R15, REG_R8, REG_R9, REG_RAX,
        REG_RBP, REG_RBX, REG_RCX, REG_RDI, REG_RDX, REG_RIP, REG_RSI, REG_RSP,
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
    known_registers[X64Register::GPR(GPR::R15).to_index().0] = Some(gregs[REG_R15 as usize] as _);
    known_registers[X64Register::GPR(GPR::R14).to_index().0] = Some(gregs[REG_R14 as usize] as _);
    known_registers[X64Register::GPR(GPR::R13).to_index().0] = Some(gregs[REG_R13 as usize] as _);
    known_registers[X64Register::GPR(GPR::R12).to_index().0] = Some(gregs[REG_R12 as usize] as _);
    known_registers[X64Register::GPR(GPR::R11).to_index().0] = Some(gregs[REG_R11 as usize] as _);
    known_registers[X64Register::GPR(GPR::R10).to_index().0] = Some(gregs[REG_R10 as usize] as _);
    known_registers[X64Register::GPR(GPR::R9).to_index().0] = Some(gregs[REG_R9 as usize] as _);
    known_registers[X64Register::GPR(GPR::R8).to_index().0] = Some(gregs[REG_R8 as usize] as _);
    known_registers[X64Register::GPR(GPR::RSI).to_index().0] = Some(gregs[REG_RSI as usize] as _);
    known_registers[X64Register::GPR(GPR::RDI).to_index().0] = Some(gregs[REG_RDI as usize] as _);
    known_registers[X64Register::GPR(GPR::RDX).to_index().0] = Some(gregs[REG_RDX as usize] as _);
    known_registers[X64Register::GPR(GPR::RCX).to_index().0] = Some(gregs[REG_RCX as usize] as _);
    known_registers[X64Register::GPR(GPR::RBX).to_index().0] = Some(gregs[REG_RBX as usize] as _);
    known_registers[X64Register::GPR(GPR::RAX).to_index().0] = Some(gregs[REG_RAX as usize] as _);

    known_registers[X64Register::GPR(GPR::RBP).to_index().0] = Some(gregs[REG_RBP as usize] as _);
    known_registers[X64Register::GPR(GPR::RSP).to_index().0] = Some(gregs[REG_RSP as usize] as _);

    // TODO: XMM registers

    FaultInfo {
        faulting_addr: si_addr as usize as _,
        ip: gregs[REG_RIP as usize] as _,
        known_registers,
    }
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
pub unsafe fn get_fault_info(siginfo: *const c_void, ucontext: *const c_void) -> FaultInfo {
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
