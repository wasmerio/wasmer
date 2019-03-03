use libc::{c_void, siginfo_t};
use nix::sys::signal::{
    sigaction, SaFlags, SigAction, SigHandler, SigSet, Signal, SIGBUS, SIGFPE, SIGILL, SIGSEGV,
};

/// `__register_frame` and `__deregister_frame` on macos take a single fde as an
/// argument, so we need to parse the fde table here.
///
/// This is a pretty direct port of llvm's fde handling code:
///     https://llvm.org/doxygen/RTDyldMemoryManager_8cpp_source.html.
#[cfg(target_os = "macos")]
pub unsafe fn visit_fde(addr: *mut u8, size: usize, visitor: extern "C" fn(*mut u8)) {
    unsafe fn process_fde(entry: *mut u8, visitor: extern "C" fn(*mut u8)) -> *mut u8 {
        let mut p = entry;
        let length = (p as *const u32).read_unaligned();
        p = p.add(4);
        let offset = (p as *const u32).read_unaligned();

        if offset != 0 {
            visitor(entry);
        }
        p.add(length as usize)
    }

    let mut p = addr;
    let end = p.add(size);

    loop {
        if p >= end {
            break;
        }

        p = process_fde(p, visitor);
    }
}

#[cfg(not(target_os = "macos"))]
pub unsafe fn visit_fde(addr: *mut u8, size: usize, visitor: extern "C" fn(*mut u8)) {
    visitor(addr);
}

extern "C" {
    fn throw_trap(ty: i32) -> !;
}

pub unsafe fn install_signal_handler() {
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
    ucontext: *mut c_void,
) {
    unsafe {
        /// By setting the instruction pointer of the interrupted context
        /// to `throw_trap` and the register of the first argument
        /// to the trap ID, we can approximate throwing an exception
        /// from a signal handler.
        set_context_to_throw(ucontext);
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
unsafe fn set_context_to_throw(ucontext: *mut c_void) {
    use libc::{ucontext_t, RDI, RIP};

    let ucontext = ucontext as *mut ucontext_t;
    (*ucontext).uc_mcontext.gregs[RIP as usize] = throw_trap as u64;
    (*ucontext).uc_mcontext.gregs[RDI as usize] = 2; // `MemoryOutOfBounds` variant.
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
unsafe fn set_context_to_throw(ucontext: *mut c_void) {
    #[allow(dead_code)]
    #[repr(C)]
    struct ucontext_t {
        uc_onstack: u32,
        uc_sigmask: u32,
        uc_stack: libc::stack_t,
        uc_link: *const ucontext_t,
        uc_mcsize: u64,
        uc_mcontext: *mut mcontext_t,
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

    let ucontext = ucontext as *mut ucontext_t;
    (*(*ucontext).uc_mcontext).ss.rip = throw_trap as u64;
    (*(*ucontext).uc_mcontext).ss.rdi = 2; // `MemoryOutOfBounds` variant.
}

#[cfg(not(any(
    all(target_os = "macos", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64"),
)))]
compile_error!("This crate doesn't yet support compiling on operating systems other than linux and macos and architectures other than x86_64");
