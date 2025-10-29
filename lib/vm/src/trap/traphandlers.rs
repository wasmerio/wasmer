// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

#![allow(static_mut_refs)]

//! WebAssembly trap handling, which is built on top of the lower-level
//! signalhandling mechanisms.

use crate::vmcontext::{VMFunctionContext, VMTrampoline};
use crate::{Trap, VMContext, VMFunctionBody};
use backtrace::Backtrace;
use core::ptr::{read, read_unaligned};
use corosensei::stack::DefaultStack;
use corosensei::trap::{CoroutineTrapHandler, TrapHandlerRegs};
use corosensei::{Coroutine, CoroutineResult, Yielder};
use scopeguard::defer;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::mem;
#[cfg(unix)]
use std::mem::MaybeUninit;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicPtr, AtomicU64, AtomicUsize, Ordering, compiler_fence};
use std::sync::{LazyLock, Once, RwLock};
use wasmer_types::TrapCode;

/// Configuration for the runtime VM
/// Currently only the stack size is configurable
pub struct VMConfig {
    /// Optional stack size (in byte) of the VM. Value lower than 8K will be rounded to 8K.
    pub wasm_stack_size: Option<usize>,
}

// TrapInformation can be stored in the "Undefined Instruction" itself.
// On x86_64, 0xC? select a "Register" for the Mod R/M part of "ud1" (so with no other bytes after)
// On Arm64, the udf alows for a 16bits values, so we'll use the same 0xC? to store the trapinfo
static MAGIC: u8 = 0xc0;

static DEFAULT_STACK_SIZE: AtomicUsize = AtomicUsize::new(1024 * 1024);

// Current definition of `ucontext_t` in the `libc` crate is incorrect
// on aarch64-apple-drawin so it's defined here with a more accurate definition.
#[repr(C)]
#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
#[allow(non_camel_case_types)]
struct ucontext_t {
    uc_onstack: libc::c_int,
    uc_sigmask: libc::sigset_t,
    uc_stack: libc::stack_t,
    uc_link: *mut libc::ucontext_t,
    uc_mcsize: usize,
    uc_mcontext: libc::mcontext_t,
}

// Current definition of `ucontext_t` in the `libc` crate is not present
// on aarch64-unknown-freebsd so it's defined here.
#[repr(C)]
#[cfg(all(target_arch = "aarch64", target_os = "freebsd"))]
#[allow(non_camel_case_types)]
struct ucontext_t {
    uc_sigmask: libc::sigset_t,
    uc_mcontext: libc::mcontext_t,
    uc_link: *mut ucontext_t,
    uc_stack: libc::stack_t,
    uc_flags: libc::c_int,
    spare: [libc::c_int; 4],
}

#[cfg(all(
    unix,
    not(all(target_arch = "aarch64", target_os = "macos")),
    not(all(target_arch = "aarch64", target_os = "freebsd"))
))]
use libc::ucontext_t;

/// Default stack size is 1MB.
pub fn set_stack_size(size: usize) {
    DEFAULT_STACK_SIZE.store(size.clamp(8 * 1024, 100 * 1024 * 1024), Ordering::Relaxed);
}

cfg_if::cfg_if! {
    if #[cfg(unix)] {
        /// Function which may handle custom signals while processing traps.
        pub type TrapHandlerFn<'a> = dyn Fn(libc::c_int, *const libc::siginfo_t, *const libc::c_void) -> bool + Send + Sync + 'a;
    } else if #[cfg(target_os = "windows")] {
        /// Function which may handle custom signals while processing traps.
        pub type TrapHandlerFn<'a> = dyn Fn(*mut windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS) -> bool + Send + Sync + 'a;
    }
}

// Process an IllegalOpcode to see if it has a TrapCode payload
unsafe fn process_illegal_op(addr: usize) -> Option<TrapCode> {
    unsafe {
        let mut val: Option<u8> = None;
        if cfg!(target_arch = "x86_64") {
            val = if read(addr as *mut u8) & 0xf0 == 0x40
                && read((addr + 1) as *mut u8) == 0x0f
                && read((addr + 2) as *mut u8) == 0xb9
            {
                Some(read((addr + 3) as *mut u8))
            } else if read(addr as *mut u8) == 0x0f && read((addr + 1) as *mut u8) == 0xb9 {
                Some(read((addr + 2) as *mut u8))
            } else {
                None
            }
        }
        if cfg!(target_arch = "aarch64") {
            val = if read_unaligned(addr as *mut u32) & 0xffff0000 == 0 {
                Some(read(addr as *mut u8))
            } else {
                None
            }
        }
        match val.and_then(|val| {
            if val & MAGIC == MAGIC {
                Some(val & 0xf)
            } else {
                None
            }
        }) {
            None => None,
            Some(val) => match val {
                0 => Some(TrapCode::StackOverflow),
                1 => Some(TrapCode::HeapAccessOutOfBounds),
                2 => Some(TrapCode::HeapMisaligned),
                3 => Some(TrapCode::TableAccessOutOfBounds),
                4 => Some(TrapCode::IndirectCallToNull),
                5 => Some(TrapCode::BadSignature),
                6 => Some(TrapCode::IntegerOverflow),
                7 => Some(TrapCode::IntegerDivisionByZero),
                8 => Some(TrapCode::BadConversionToInteger),
                9 => Some(TrapCode::UnreachableCodeReached),
                10 => Some(TrapCode::UnalignedAtomic),
                _ => None,
            },
        }
    }
}

cfg_if::cfg_if! {
    if #[cfg(unix)] {
        static mut PREV_SIGSEGV: MaybeUninit<libc::sigaction> = MaybeUninit::uninit();
        static mut PREV_SIGBUS: MaybeUninit<libc::sigaction> = MaybeUninit::uninit();
        static mut PREV_SIGILL: MaybeUninit<libc::sigaction> = MaybeUninit::uninit();
        static mut PREV_SIGFPE: MaybeUninit<libc::sigaction> = MaybeUninit::uninit();

        unsafe fn platform_init() { unsafe {
            let register = |slot: &mut MaybeUninit<libc::sigaction>, signal: i32| {
                let mut handler: libc::sigaction = mem::zeroed();
                // The flags here are relatively careful, and they are...
                //
                // SA_SIGINFO gives us access to information like the program
                // counter from where the fault happened.
                //
                // SA_ONSTACK allows us to handle signals on an alternate stack,
                // so that the handler can run in response to running out of
                // stack space on the main stack. Rust installs an alternate
                // stack with sigaltstack, so we rely on that.
                //
                // SA_NODEFER allows us to reenter the signal handler if we
                // crash while handling the signal, and fall through to the
                // Breakpad handler by testing handlingSegFault.
                handler.sa_flags = libc::SA_SIGINFO | libc::SA_NODEFER | libc::SA_ONSTACK;
                handler.sa_sigaction = trap_handler as usize;
                libc::sigemptyset(&mut handler.sa_mask);
                if libc::sigaction(signal, &handler, slot.as_mut_ptr()) != 0 {
                    panic!(
                        "unable to install signal handler: {}",
                        io::Error::last_os_error(),
                    );
                }
            };

            // Allow handling OOB with signals on all architectures
            register(&mut PREV_SIGSEGV, libc::SIGSEGV);

            // Handle `unreachable` instructions which execute `ud2` right now
            register(&mut PREV_SIGILL, libc::SIGILL);

            // x86 uses SIGFPE to report division by zero
            if cfg!(target_arch = "x86") || cfg!(target_arch = "x86_64") {
                register(&mut PREV_SIGFPE, libc::SIGFPE);
            }

            // On ARM, handle Unaligned Accesses.
            // On Darwin, guard page accesses are raised as SIGBUS.
            if cfg!(target_arch = "arm") || cfg!(target_vendor = "apple") {
                register(&mut PREV_SIGBUS, libc::SIGBUS);
            }

            // This is necessary to support debugging under LLDB on Darwin.
            // For more details see https://github.com/mono/mono/commit/8e75f5a28e6537e56ad70bf870b86e22539c2fb7
            #[cfg(target_vendor = "apple")]
            {
                use mach2::exception_types::*;
                use mach2::kern_return::*;
                use mach2::port::*;
                use mach2::thread_status::*;
                use mach2::traps::*;
                use mach2::mach_types::*;

                unsafe extern "C" {
                    fn task_set_exception_ports(
                        task: task_t,
                        exception_mask: exception_mask_t,
                        new_port: mach_port_t,
                        behavior: exception_behavior_t,
                        new_flavor: thread_state_flavor_t,
                    ) -> kern_return_t;
                }

                #[allow(non_snake_case)]
                #[cfg(target_arch = "x86_64")]
                let MACHINE_THREAD_STATE = x86_THREAD_STATE64;
                #[allow(non_snake_case)]
                #[cfg(target_arch = "aarch64")]
                let MACHINE_THREAD_STATE = 6;

                task_set_exception_ports(
                    mach_task_self(),
                    EXC_MASK_BAD_ACCESS | EXC_MASK_ARITHMETIC | EXC_MASK_BAD_INSTRUCTION,
                    MACH_PORT_NULL,
                    EXCEPTION_STATE_IDENTITY as exception_behavior_t,
                    MACHINE_THREAD_STATE,
                );
            }
        }}

        unsafe extern "C" fn trap_handler(
            signum: libc::c_int,
            siginfo: *mut libc::siginfo_t,
            context: *mut libc::c_void,
        ) { unsafe {
            let previous = match signum {
                libc::SIGSEGV => &PREV_SIGSEGV,
                libc::SIGBUS => &PREV_SIGBUS,
                libc::SIGFPE => &PREV_SIGFPE,
                libc::SIGILL => &PREV_SIGILL,
                _ => panic!("unknown signal: {signum}"),
            };
            // We try to get the fault address associated to this signal
            let maybe_fault_address = match signum {
                libc::SIGSEGV | libc::SIGBUS => {
                    Some((*siginfo).si_addr() as usize)
                }
                _ => None,
            };
            let trap_code = match signum {
                // check if it was cased by a UD and if the Trap info is a payload to it
                libc::SIGILL => {
                    let addr = (*siginfo).si_addr() as usize;
                    process_illegal_op(addr)
                }
                _ => None,
            };
            let ucontext = &mut *(context as *mut ucontext_t);
            let (pc, sp) = get_pc_sp(ucontext);
            let handled = TrapHandlerContext::handle_trap(
                pc,
                sp,
                maybe_fault_address,
                trap_code,
                |regs| update_context(ucontext, regs),
                |handler| handler(signum, siginfo, context),
            );

            if handled {
                return;
            }

            // This signal is not for any compiled wasm code we expect, so we
            // need to forward the signal to the next handler. If there is no
            // next handler (SIG_IGN or SIG_DFL), then it's time to crash. To do
            // this, we set the signal back to its original disposition and
            // return. This will cause the faulting op to be re-executed which
            // will crash in the normal way. If there is a next handler, call
            // it. It will either crash synchronously, fix up the instruction
            // so that execution can continue and return, or trigger a crash by
            // returning the signal to it's original disposition and returning.
            let previous = &*previous.as_ptr();
            if previous.sa_flags & libc::SA_SIGINFO != 0 {
                mem::transmute::<
                    usize,
                    extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void),
                >(previous.sa_sigaction)(signum, siginfo, context)
            } else if previous.sa_sigaction == libc::SIG_DFL
            {
                libc::sigaction(signum, previous, ptr::null_mut());
            } else if previous.sa_sigaction != libc::SIG_IGN {
                mem::transmute::<usize, extern "C" fn(libc::c_int)>(
                    previous.sa_sigaction
                )(signum)
            }
        }}

        unsafe fn get_pc_sp(context: &ucontext_t) -> (usize, usize) {
            let (pc, sp);
            cfg_if::cfg_if! {
                if #[cfg(all(
                    any(target_os = "linux", target_os = "android"),
                    target_arch = "x86_64",
                ))] {
                    pc = context.uc_mcontext.gregs[libc::REG_RIP as usize] as usize;
                    sp = context.uc_mcontext.gregs[libc::REG_RSP as usize] as usize;
                } else if #[cfg(all(
                    any(target_os = "linux", target_os = "android"),
                    target_arch = "x86",
                ))] {
                    pc = context.uc_mcontext.gregs[libc::REG_EIP as usize] as usize;
                    sp = context.uc_mcontext.gregs[libc::REG_ESP as usize] as usize;
                } else if #[cfg(all(target_os = "freebsd", target_arch = "x86"))] {
                    pc = context.uc_mcontext.mc_eip as usize;
                    sp = context.uc_mcontext.mc_esp as usize;
                } else if #[cfg(all(target_os = "freebsd", target_arch = "x86_64"))] {
                    pc = context.uc_mcontext.mc_rip as usize;
                    sp = context.uc_mcontext.mc_rsp as usize;
                } else if #[cfg(all(target_vendor = "apple", target_arch = "x86_64"))] {
                    let mcontext = unsafe { &*context.uc_mcontext };
                    pc = mcontext.__ss.__rip as usize;
                    sp = mcontext.__ss.__rsp as usize;
                } else if #[cfg(all(
                        any(target_os = "linux", target_os = "android"),
                        target_arch = "aarch64",
                    ))] {
                    pc = context.uc_mcontext.pc as usize;
                    sp = context.uc_mcontext.sp as usize;
                } else if #[cfg(all(
                    any(target_os = "linux", target_os = "android"),
                    target_arch = "arm",
                ))] {
                    pc = context.uc_mcontext.arm_pc as usize;
                    sp = context.uc_mcontext.arm_sp as usize;
                } else if #[cfg(all(
                    any(target_os = "linux", target_os = "android"),
                    any(target_arch = "riscv64", target_arch = "riscv32"),
                ))] {
                    pc = context.uc_mcontext.__gregs[libc::REG_PC] as usize;
                    sp = context.uc_mcontext.__gregs[libc::REG_SP] as usize;
                } else if #[cfg(all(target_vendor = "apple", target_arch = "aarch64"))] {
                    let mcontext = unsafe { &*context.uc_mcontext };
                    pc = mcontext.__ss.__pc as usize;
                    sp = mcontext.__ss.__sp as usize;
                } else if #[cfg(all(target_os = "freebsd", target_arch = "aarch64"))] {
                    pc = context.uc_mcontext.mc_gpregs.gp_elr as usize;
                    sp = context.uc_mcontext.mc_gpregs.gp_sp as usize;
                } else if #[cfg(all(target_os = "linux", target_arch = "loongarch64"))] {
                    pc = context.uc_mcontext.__gregs[1] as usize;
                    sp = context.uc_mcontext.__gregs[3] as usize;
                } else {
                    compile_error!("Unsupported platform");
                }
            };
            (pc, sp)
        }

        unsafe fn update_context(context: &mut ucontext_t, regs: TrapHandlerRegs) {
            cfg_if::cfg_if! {
                if #[cfg(all(
                        any(target_os = "linux", target_os = "android"),
                        target_arch = "x86_64",
                    ))] {
                    let TrapHandlerRegs { rip, rsp, rbp, rdi, rsi } = regs;
                    context.uc_mcontext.gregs[libc::REG_RIP as usize] = rip as i64;
                    context.uc_mcontext.gregs[libc::REG_RSP as usize] = rsp as i64;
                    context.uc_mcontext.gregs[libc::REG_RBP as usize] = rbp as i64;
                    context.uc_mcontext.gregs[libc::REG_RDI as usize] = rdi as i64;
                    context.uc_mcontext.gregs[libc::REG_RSI as usize] = rsi as i64;
                } else if #[cfg(all(
                    any(target_os = "linux", target_os = "android"),
                    target_arch = "x86",
                ))] {
                    let TrapHandlerRegs { eip, esp, ebp, ecx, edx } = regs;
                    context.uc_mcontext.gregs[libc::REG_EIP as usize] = eip as i32;
                    context.uc_mcontext.gregs[libc::REG_ESP as usize] = esp as i32;
                    context.uc_mcontext.gregs[libc::REG_EBP as usize] = ebp as i32;
                    context.uc_mcontext.gregs[libc::REG_ECX as usize] = ecx as i32;
                    context.uc_mcontext.gregs[libc::REG_EDX as usize] = edx as i32;
                } else if #[cfg(all(target_vendor = "apple", target_arch = "x86_64"))] {
                    let TrapHandlerRegs { rip, rsp, rbp, rdi, rsi } = regs;
                    let mcontext = unsafe { &mut *context.uc_mcontext };
                    mcontext.__ss.__rip = rip;
                    mcontext.__ss.__rsp = rsp;
                    mcontext.__ss.__rbp = rbp;
                    mcontext.__ss.__rdi = rdi;
                    mcontext.__ss.__rsi = rsi;
                } else if #[cfg(all(target_os = "freebsd", target_arch = "x86"))] {
                    let TrapHandlerRegs { eip, esp, ebp, ecx, edx } = regs;
                    context.uc_mcontext.mc_eip = eip as libc::register_t;
                    context.uc_mcontext.mc_esp = esp as libc::register_t;
                    context.uc_mcontext.mc_ebp = ebp as libc::register_t;
                    context.uc_mcontext.mc_ecx = ecx as libc::register_t;
                    context.uc_mcontext.mc_edx = edx as libc::register_t;
                } else if #[cfg(all(target_os = "freebsd", target_arch = "x86_64"))] {
                    let TrapHandlerRegs { rip, rsp, rbp, rdi, rsi } = regs;
                    context.uc_mcontext.mc_rip = rip as libc::register_t;
                    context.uc_mcontext.mc_rsp = rsp as libc::register_t;
                    context.uc_mcontext.mc_rbp = rbp as libc::register_t;
                    context.uc_mcontext.mc_rdi = rdi as libc::register_t;
                    context.uc_mcontext.mc_rsi = rsi as libc::register_t;
                } else if #[cfg(all(
                        any(target_os = "linux", target_os = "android"),
                        target_arch = "aarch64",
                    ))] {
                    let TrapHandlerRegs { pc, sp, x0, x1, x29, lr } = regs;
                    context.uc_mcontext.pc = pc;
                    context.uc_mcontext.sp = sp;
                    context.uc_mcontext.regs[0] = x0;
                    context.uc_mcontext.regs[1] = x1;
                    context.uc_mcontext.regs[29] = x29;
                    context.uc_mcontext.regs[30] = lr;
                } else if #[cfg(all(
                        any(target_os = "linux", target_os = "android"),
                        target_arch = "arm",
                    ))] {
                    let TrapHandlerRegs {
                        pc,
                        r0,
                        r1,
                        r7,
                        r11,
                        r13,
                        r14,
                        cpsr_thumb,
                        cpsr_endian,
                    } = regs;
                    context.uc_mcontext.arm_pc = pc;
                    context.uc_mcontext.arm_r0 = r0;
                    context.uc_mcontext.arm_r1 = r1;
                    context.uc_mcontext.arm_r7 = r7;
                    context.uc_mcontext.arm_fp = r11;
                    context.uc_mcontext.arm_sp = r13;
                    context.uc_mcontext.arm_lr = r14;
                    if cpsr_thumb {
                        context.uc_mcontext.arm_cpsr |= 0x20;
                    } else {
                        context.uc_mcontext.arm_cpsr &= !0x20;
                    }
                    if cpsr_endian {
                        context.uc_mcontext.arm_cpsr |= 0x200;
                    } else {
                        context.uc_mcontext.arm_cpsr &= !0x200;
                    }
                } else if #[cfg(all(
                    any(target_os = "linux", target_os = "android"),
                    any(target_arch = "riscv64", target_arch = "riscv32"),
                ))] {
                    let TrapHandlerRegs { pc, ra, sp, a0, a1, s0 } = regs;
                    context.uc_mcontext.__gregs[libc::REG_PC] = pc as libc::c_ulong;
                    context.uc_mcontext.__gregs[libc::REG_RA] = ra as libc::c_ulong;
                    context.uc_mcontext.__gregs[libc::REG_SP] = sp as libc::c_ulong;
                    context.uc_mcontext.__gregs[libc::REG_A0] = a0 as libc::c_ulong;
                    context.uc_mcontext.__gregs[libc::REG_A0 + 1] = a1 as libc::c_ulong;
                    context.uc_mcontext.__gregs[libc::REG_S0] = s0 as libc::c_ulong;
                } else if #[cfg(all(target_vendor = "apple", target_arch = "aarch64"))] {
                    let TrapHandlerRegs { pc, sp, x0, x1, x29, lr } = regs;
                    let mcontext = unsafe { &mut *context.uc_mcontext };
                    mcontext.__ss.__pc = pc;
                    mcontext.__ss.__sp = sp;
                    mcontext.__ss.__x[0] = x0;
                    mcontext.__ss.__x[1] = x1;
                    mcontext.__ss.__fp = x29;
                    mcontext.__ss.__lr = lr;
                } else if #[cfg(all(target_os = "freebsd", target_arch = "aarch64"))] {
                    let TrapHandlerRegs { pc, sp, x0, x1, x29, lr } = regs;
                    context.uc_mcontext.mc_gpregs.gp_elr = pc as libc::register_t;
                    context.uc_mcontext.mc_gpregs.gp_sp = sp as libc::register_t;
                    context.uc_mcontext.mc_gpregs.gp_x[0] = x0 as libc::register_t;
                    context.uc_mcontext.mc_gpregs.gp_x[1] = x1 as libc::register_t;
                    context.uc_mcontext.mc_gpregs.gp_x[29] = x29 as libc::register_t;
                    context.uc_mcontext.mc_gpregs.gp_x[30] = lr as libc::register_t;
                } else if #[cfg(all(target_os = "linux", target_arch = "loongarch64"))] {
                    let TrapHandlerRegs { pc, sp, a0, a1, fp, ra } = regs;
                    context.uc_mcontext.__pc = pc;
                    context.uc_mcontext.__gregs[1] = ra;
                    context.uc_mcontext.__gregs[3] = sp;
                    context.uc_mcontext.__gregs[4] = a0;
                    context.uc_mcontext.__gregs[5] = a1;
                    context.uc_mcontext.__gregs[22] = fp;
                } else {
                    compile_error!("Unsupported platform");
                }
            };
        }
    } else if #[cfg(target_os = "windows")] {
        use windows_sys::Win32::System::Diagnostics::Debug::{
            AddVectoredExceptionHandler,
            CONTEXT,
            EXCEPTION_CONTINUE_EXECUTION,
            EXCEPTION_CONTINUE_SEARCH,
            EXCEPTION_POINTERS,
        };
        use windows_sys::Win32::Foundation::{
            EXCEPTION_ACCESS_VIOLATION,
            EXCEPTION_ILLEGAL_INSTRUCTION,
            EXCEPTION_INT_DIVIDE_BY_ZERO,
            EXCEPTION_INT_OVERFLOW,
            EXCEPTION_STACK_OVERFLOW,
        };

        unsafe fn platform_init() {
            unsafe {
                // our trap handler needs to go first, so that we can recover from
                // wasm faults and continue execution, so pass `1` as a true value
                // here.
                let handler = AddVectoredExceptionHandler(1, Some(exception_handler));
                if handler.is_null() {
                    panic!("failed to add exception handler: {}", io::Error::last_os_error());
                }
            }
        }

        unsafe extern "system" fn exception_handler(
            exception_info: *mut EXCEPTION_POINTERS
        ) -> i32 {
            unsafe {
                // Check the kind of exception, since we only handle a subset within
                // wasm code. If anything else happens we want to defer to whatever
                // the rest of the system wants to do for this exception.
                let record = &*(*exception_info).ExceptionRecord;
                if record.ExceptionCode != EXCEPTION_ACCESS_VIOLATION &&
                    record.ExceptionCode != EXCEPTION_ILLEGAL_INSTRUCTION &&
                    record.ExceptionCode != EXCEPTION_STACK_OVERFLOW &&
                    record.ExceptionCode != EXCEPTION_INT_DIVIDE_BY_ZERO &&
                    record.ExceptionCode != EXCEPTION_INT_OVERFLOW
                {
                    return EXCEPTION_CONTINUE_SEARCH;
                }

                // FIXME: this is what the previous C++ did to make sure that TLS
                // works by the time we execute this trap handling code. This isn't
                // exactly super easy to call from Rust though and it's not clear we
                // necessarily need to do so. Leaving this here in case we need this
                // in the future, but for now we can probably wait until we see a
                // strange fault before figuring out how to reimplement this in
                // Rust.
                //
                // if (!NtCurrentTeb()->Reserved1[sThreadLocalArrayPointerIndex]) {
                //     return EXCEPTION_CONTINUE_SEARCH;
                // }

                let context = &mut *(*exception_info).ContextRecord;
                let (pc, sp) = get_pc_sp(context);

                // We try to get the fault address associated to this exception.
                let maybe_fault_address = match record.ExceptionCode {
                    EXCEPTION_ACCESS_VIOLATION => Some(record.ExceptionInformation[1]),
                    EXCEPTION_STACK_OVERFLOW => Some(sp),
                    _ => None,
                };
                let trap_code = match record.ExceptionCode {
                    // check if it was cased by a UD and if the Trap info is a payload to it
                    EXCEPTION_ILLEGAL_INSTRUCTION => {
                        process_illegal_op(pc)
                    }
                    _ => None,
                };
                // This is basically the same as the unix version above, only with a
                // few parameters tweaked here and there.
                let handled = TrapHandlerContext::handle_trap(
                    pc,
                    sp,
                    maybe_fault_address,
                    trap_code,
                    |regs| update_context(context, regs),
                    |handler| handler(exception_info),
                );

                if handled {
                    EXCEPTION_CONTINUE_EXECUTION
                } else {
                    EXCEPTION_CONTINUE_SEARCH
                }
            }
        }

        unsafe fn get_pc_sp(context: &CONTEXT) -> (usize, usize) {
            let (pc, sp);
            cfg_if::cfg_if! {
                if #[cfg(target_arch = "x86_64")] {
                    pc = context.Rip as usize;
                    sp = context.Rsp as usize;
                } else if #[cfg(target_arch = "x86")] {
                    pc = context.Rip as usize;
                    sp = context.Rsp as usize;
                } else {
                    compile_error!("Unsupported platform");
                }
            };
            (pc, sp)
        }

        unsafe fn update_context(context: &mut CONTEXT, regs: TrapHandlerRegs) {
            cfg_if::cfg_if! {
                if #[cfg(target_arch = "x86_64")] {
                    let TrapHandlerRegs { rip, rsp, rbp, rdi, rsi } = regs;
                    context.Rip = rip;
                    context.Rsp = rsp;
                    context.Rbp = rbp;
                    context.Rdi = rdi;
                    context.Rsi = rsi;
                } else if #[cfg(target_arch = "x86")] {
                    let TrapHandlerRegs { eip, esp, ebp, ecx, edx } = regs;
                    context.Eip = eip;
                    context.Esp = esp;
                    context.Ebp = ebp;
                    context.Ecx = ecx;
                    context.Edx = edx;
                } else {
                    compile_error!("Unsupported platform");
                }
            };
        }
    }
}

/// This function is required to be called before any WebAssembly is entered.
/// This will configure global state such as signal handlers to prepare the
/// process to receive wasm traps.
///
/// This function must not only be called globally once before entering
/// WebAssembly but it must also be called once-per-thread that enters
/// WebAssembly. Currently in wasmer's integration this function is called on
/// creation of a `Store`.
pub fn init_traps() {
    static INIT: Once = Once::new();
    INIT.call_once(|| unsafe {
        platform_init();
    });
}

/// Raises a user-defined trap immediately.
///
/// This function performs as-if a wasm trap was just executed, only the trap
/// has a dynamic payload associated with it which is user-provided. This trap
/// payload is then returned from `catch_traps` below.
///
/// # Safety
///
/// Only safe to call when wasm code is on the stack, aka `catch_traps` must
/// have been previous called and not yet returned.
/// Additionally no Rust destructors may be on the stack.
/// They will be skipped and not executed.
pub unsafe fn raise_user_trap(data: Box<dyn Error + Send + Sync>) -> ! {
    unsafe { unwind_with(UnwindReason::UserTrap(data)) }
    unreachable!();
}

/// Raises a trap from inside library code immediately.
///
/// This function performs as-if a wasm trap was just executed. This trap
/// payload is then returned from `catch_traps` below.
///
/// # Safety
///
/// Only safe to call when wasm code is on the stack, aka `catch_traps` must
/// have been previous called and not yet returned.
/// Additionally no Rust destructors may be on the stack.
/// They will be skipped and not executed.
pub unsafe fn raise_lib_trap(trap: Trap) {
    let is_resumable = trap.is_resumable();
    unsafe { unwind_with(UnwindReason::LibTrap(trap)) }
    if !is_resumable {
        unreachable!();
    }
}

/// Carries a Rust panic across wasm code and resumes the panic on the other
/// side.
///
/// # Safety
///
/// Only safe to call when wasm code is on the stack, aka `catch_traps` must
/// have been previously called and not returned. Additionally no Rust destructors may be on the
/// stack. They will be skipped and not executed.
pub unsafe fn resume_panic(payload: Box<dyn Any + Send>) -> ! {
    unsafe { unwind_with(UnwindReason::Panic(payload)) }
    unreachable!();
}

/// Call the wasm function pointed to by `callee`.
///
/// * `vmctx` - the callee vmctx argument
/// * `caller_vmctx` - the caller vmctx argument
/// * `trampoline` - the jit-generated trampoline whose ABI takes 4 values, the
///   callee vmctx, the caller vmctx, the `callee` argument below, and then the
///   `values_vec` argument.
/// * `callee` - the third argument to the `trampoline` function
/// * `values_vec` - points to a buffer which holds the incoming arguments, and to
///   which the outgoing return values will be written.
///
/// # Safety
///
/// Wildly unsafe because it calls raw function pointers and reads/writes raw
/// function pointers.
pub unsafe fn wasmer_call_trampoline(
    trap_handler: Option<*const TrapHandlerFn<'static>>,
    config: &VMConfig,
    vmctx: VMFunctionContext,
    trampoline: VMTrampoline,
    callee: *const VMFunctionBody,
    values_vec: *mut u8,
) -> Result<(), Trap> {
    unsafe {
        catch_traps(trap_handler, config, move || {
            mem::transmute::<
                unsafe extern "C" fn(
                    *mut VMContext,
                    *const VMFunctionBody,
                    *mut wasmer_types::RawValue,
                ),
                extern "C" fn(VMFunctionContext, *const VMFunctionBody, *mut u8),
            >(trampoline)(vmctx, callee, values_vec);
        })
    }
}

/// Call the wasm function pointed to by `callee`.
///
/// * `vmctx` - the callee vmctx argument
/// * `caller_vmctx` - the caller vmctx argument
/// * `trampoline` - the jit-generated trampoline whose ABI takes 4 values, the
///   callee vmctx, the caller vmctx, the `callee` argument below, and then the
///   `values_vec` argument.
/// * `callee` - the third argument to the `trampoline` function
/// * `values_vec` - points to a buffer which holds the incoming arguments, and to
///   which the outgoing return values will be written.
///
/// # Safety
///
/// Wildly unsafe because it calls raw function pointers and reads/writes raw
/// function pointers.
pub unsafe fn wasmer_call_trampoline_resume(
    trap_handler: Option<*const TrapHandlerFn<'static>>,
    continuation: u64,
) -> Result<(), Trap> {
    unsafe { catch_traps_resume(trap_handler, continuation) }
}

/// Catches any wasm traps that happen within the execution of `closure`,
/// returning them as a `Result`.
///
/// # Safety
///
/// Highly unsafe since `closure` won't have any dtors run.
pub unsafe fn catch_traps<F, R: 'static>(
    trap_handler: Option<*const TrapHandlerFn<'static>>,
    config: &VMConfig,
    closure: F,
) -> Result<R, Trap>
where
    F: FnOnce() -> R + 'static,
{
    // Ensure that per-thread initialization is done.
    lazy_per_thread_init()?;
    let stack_size = config
        .wasm_stack_size
        .unwrap_or_else(|| DEFAULT_STACK_SIZE.load(Ordering::Relaxed));
    on_wasm_stack(stack_size, trap_handler, closure).map_err(UnwindReason::into_trap)
}

/// Catches any wasm traps that happen within the execution of `closure`,
/// returning them as a `Result`.
///
/// # Safety
///
/// Highly unsafe since `closure` won't have any dtors run.
pub unsafe fn catch_traps_resume<R: 'static>(
    trap_handler: Option<*const TrapHandlerFn<'static>>,
    continuation: u64,
) -> Result<R, Trap> {
    // Ensure that per-thread initialization is done.
    // TODO: We don't need this
    lazy_per_thread_init()?;
    on_wasm_stack_resume::<R>(continuation, trap_handler).map_err(UnwindReason::into_trap)
}

// We need two separate thread-local variables here:
// - YIELDER is set within the new stack and is used to unwind back to the root
//   of the stack from inside it.
// - TRAP_HANDLER is set from outside the new stack and is solely used from
//   signal handlers. It must be atomic since it is used by signal handlers.
//
// We also do per-thread signal stack initialization on the first time
// TRAP_HANDLER is accessed.
thread_local! {
    static YIELDER: Cell<Option<NonNull<Yielder<(), UnwindReason>>>> = const { Cell::new(None) };
    static TRAP_HANDLER: AtomicPtr<TrapHandlerContext> = const { AtomicPtr::new(ptr::null_mut()) };
    static ACTIVE_STACKS: RefCell<BTreeMap<u64, ErasedContinuation>> = const { RefCell::new(BTreeMap::new()) };
    static NEXT_STACK_ID: AtomicU64 = const { AtomicU64::new(0) };
}

/// Read-only information that is used by signal handlers to handle and recover
/// from traps.
#[allow(clippy::type_complexity)]
struct TrapHandlerContext {
    inner: *const u8,
    handle_trap: fn(
        *const u8,
        usize,
        usize,
        Option<usize>,
        Option<TrapCode>,
        &mut dyn FnMut(TrapHandlerRegs),
    ) -> bool,
    custom_trap: Option<*const TrapHandlerFn<'static>>,
}
struct TrapHandlerContextInner<T> {
    /// Information about the currently running coroutine. This is used to
    /// reset execution to the root of the coroutine when a trap is handled.
    coro_trap_handler: CoroutineTrapHandler<Result<T, UnwindReason>>,
}

impl TrapHandlerContext {
    /// Runs the given function with a trap handler context. The previous
    /// trap handler context is preserved and restored afterwards.
    fn install<T, R>(
        custom_trap: Option<*const TrapHandlerFn<'static>>,
        coro_trap_handler: CoroutineTrapHandler<Result<T, UnwindReason>>,
        f: impl FnOnce() -> R,
    ) -> R {
        // Type-erase the trap handler function so that it can be placed in TLS.
        fn func<T>(
            ptr: *const u8,
            pc: usize,
            sp: usize,
            maybe_fault_address: Option<usize>,
            trap_code: Option<TrapCode>,
            update_regs: &mut dyn FnMut(TrapHandlerRegs),
        ) -> bool {
            unsafe {
                (*(ptr as *const TrapHandlerContextInner<T>)).handle_trap(
                    pc,
                    sp,
                    maybe_fault_address,
                    trap_code,
                    update_regs,
                )
            }
        }
        let inner = TrapHandlerContextInner { coro_trap_handler };
        let ctx = Self {
            inner: &inner as *const _ as *const u8,
            handle_trap: func::<T>,
            custom_trap,
        };

        compiler_fence(Ordering::Release);
        let prev = TRAP_HANDLER.with(|ptr| {
            let prev = ptr.load(Ordering::Relaxed);
            ptr.store(&ctx as *const Self as *mut Self, Ordering::Relaxed);
            prev
        });

        defer! {
            TRAP_HANDLER.with(|ptr| ptr.store(prev, Ordering::Relaxed));
            compiler_fence(Ordering::Acquire);
        }

        f()
    }

    /// Attempts to handle the trap if it's a wasm trap.
    unsafe fn handle_trap(
        pc: usize,
        sp: usize,
        maybe_fault_address: Option<usize>,
        trap_code: Option<TrapCode>,
        mut update_regs: impl FnMut(TrapHandlerRegs),
        call_handler: impl Fn(&TrapHandlerFn<'static>) -> bool,
    ) -> bool {
        unsafe {
            let ptr = TRAP_HANDLER.with(|ptr| ptr.load(Ordering::Relaxed));
            if ptr.is_null() {
                return false;
            }

            let ctx = &*ptr;

            // Check if this trap is handled by a custom trap handler.
            if let Some(trap_handler) = ctx.custom_trap {
                if call_handler(&*trap_handler) {
                    return true;
                }
            }

            (ctx.handle_trap)(
                ctx.inner,
                pc,
                sp,
                maybe_fault_address,
                trap_code,
                &mut update_regs,
            )
        }
    }
}

impl<T> TrapHandlerContextInner<T> {
    unsafe fn handle_trap(
        &self,
        pc: usize,
        sp: usize,
        maybe_fault_address: Option<usize>,
        trap_code: Option<TrapCode>,
        update_regs: &mut dyn FnMut(TrapHandlerRegs),
    ) -> bool {
        unsafe {
            // Check if this trap occurred while executing on the Wasm stack. We can
            // only recover from traps if that is the case.
            if !self.coro_trap_handler.stack_ptr_in_bounds(sp) {
                return false;
            }

            let signal_trap = trap_code.or_else(|| {
                maybe_fault_address.map(|addr| {
                    if self.coro_trap_handler.stack_ptr_in_bounds(addr) {
                        TrapCode::StackOverflow
                    } else {
                        TrapCode::HeapAccessOutOfBounds
                    }
                })
            });

            // Don't try to generate a backtrace for stack overflows: unwinding
            // information is often not precise enough to properly describe what is
            // happenning during a function prologue, which can lead the unwinder to
            // read invalid memory addresses.
            //
            // See: https://github.com/rust-lang/backtrace-rs/pull/357
            let backtrace = if signal_trap == Some(TrapCode::StackOverflow) {
                Backtrace::from(vec![])
            } else {
                Backtrace::new_unresolved()
            };

            // Set up the register state for exception return to force the
            // coroutine to return to its caller with UnwindReason::WasmTrap.
            let unwind = UnwindReason::WasmTrap {
                backtrace,
                signal_trap,
                pc,
            };
            let regs = self
                .coro_trap_handler
                .setup_trap_handler(move || Err(unwind));
            update_regs(regs);
            true
        }
    }
}

// TODO: Make private again
/// AAAAAAAAAAa
pub enum UnwindReason {
    /// A panic caused by the host
    Panic(Box<dyn Any + Send>),
    /// A custom error triggered by the user
    UserTrap(Box<dyn Error + Send + Sync>),
    /// A Trap triggered by a wasm libcall
    LibTrap(Trap),
    /// A trap caused by the Wasm generated code
    WasmTrap {
        // TODO: Make private again
        /// AAAAAAAAAAa
        backtrace: Backtrace,
        // TODO: Make private again
        /// AAAAAAAAAAa
        pc: usize,
        // TODO: Make private again
        /// AAAAAAAAAAa
        signal_trap: Option<TrapCode>,
    },
}

impl std::fmt::Debug for UnwindReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UserTrap(data) => f
                .debug_tuple("UnwindReason::UserTrap")
                .field(&data.to_string())
                .finish(),
            Self::LibTrap(trap) => f.debug_tuple("UnwindReason::LibTrap").field(trap).finish(),
            Self::WasmTrap {
                pc, signal_trap, ..
            } => f
                .debug_struct("UnwindReason::WasmTrap")
                .field("pc", pc)
                .field("signal_trap", signal_trap)
                .finish(),
            Self::Panic(_) => f.debug_tuple("UnwindReason::Panic").finish(),
        }
    }
}

impl UnwindReason {
    fn into_trap(self) -> Trap {
        match self {
            Self::UserTrap(data) => Trap::User(data),
            Self::LibTrap(trap) => trap,
            Self::WasmTrap {
                backtrace,
                pc,
                signal_trap,
            } => Trap::wasm(pc, backtrace, signal_trap),
            Self::Panic(panic) => std::panic::resume_unwind(panic),
        }
    }
    fn is_resumable(&self) -> bool {
        match self {
            Self::LibTrap(trap) => trap.is_resumable(),
            _ => false,
        }
    }
}

unsafe fn unwind_with(reason: UnwindReason) {
    unsafe {
        let yielder = YIELDER
            .with(|cell| cell.get())
            .expect("Can only unwind on wasm stack");

        // TODO: Remove this, as it is only here for the unreachable assertion below
        let is_resumable = reason.is_resumable();
        yielder.as_ref().suspend(reason);

        // on_wasm_stack will forcibly reset the coroutine stack after yielding.
        if !is_resumable {
            unreachable!("suspend resumed after a non-resumable trap");
        }
    }
}

/// A reference to a continuation in ACTIVE_STACKS
struct ContinuationRef {
    id: u64,
}
impl ContinuationRef {
    fn new<T: 'static, F: FnOnce() -> T + 'static>(f: F, stack_size: usize) -> Self {
        let id = NEXT_STACK_ID.with(|cell| cell.fetch_add(1, Ordering::Relaxed));
        let boxed_coro = Continuation::new(f, stack_size);
        ACTIVE_STACKS.with_borrow_mut(|stacks| stacks.insert(id, boxed_coro.into()));
        Self { id }
    }
    fn resume<T: 'static>(
        &self,
        trap_handler: Option<*const TrapHandlerFn<'static>>,
    ) -> Result<T, UnwindReason> {
        let id = self.id;
        let erased_continuation = ACTIVE_STACKS
            .with_borrow_mut(|map| map.remove(&id))
            .unwrap();
        // TODO: Handle the unwrap above better

        let continuation: Continuation<T> = erased_continuation.into();
        let (mut maybe_continuation, mut result) = continuation.resume(trap_handler);
        if let Err(e) = &mut result {
            if let UnwindReason::LibTrap(Trap::Continuation {
                continuation_ref, ..
            }) = e
            {
                // TODO: Assert that all resumable traps end up here
                continuation_ref.replace(id);
            }
        }
        if let Some(cont) = maybe_continuation.take() {
            // Put the continuation back into ACTIVE_STACKS
            ACTIVE_STACKS.with_borrow_mut(|map| map.insert(id, cont.into()));
        }
        result
    }
}

struct StackPool {
    pool: RwLock<BTreeMap<usize, crossbeam_queue::SegQueue<DefaultStack>>>,
}

impl StackPool {
    fn new() -> Self {
        Self {
            pool: Default::default(),
        }
    }
    /// Get a stack from the stack pool or create a new one if none is available
    ///
    /// Tries to keep the stack size as close as possible to the requested size
    fn get_stack(&self, stack_size: usize) -> DefaultStack {
        {
            let pool_guard = self.pool.read().unwrap();
            if let Some(existing_queue) = pool_guard.get(&stack_size) {
                return existing_queue
                    .pop()
                    .unwrap_or_else(|| DefaultStack::new(stack_size).unwrap());
            };
        }
        DefaultStack::new(stack_size).unwrap()
    }
    /// Return a stack to the pool
    ///
    /// The stack size needs to be provided, because we can not get the stack size from the stack itself
    fn return_stack(&self, stack: DefaultStack, stack_size: usize) {
        {
            let pool_guard = self.pool.read().unwrap();
            if let Some(existing_queue) = pool_guard.get(&stack_size) {
                existing_queue.push(stack);
                return;
            };
        }
        let mut pool_guard = self.pool.write().unwrap();
        let new_queue = crossbeam_queue::SegQueue::new();
        new_queue.push(stack);
        pool_guard.insert(stack_size, new_queue);
    }
}

struct Continuation<T: 'static> {
    coro: Coroutine<(), UnwindReason, Result<T, UnwindReason>>,
    /// The yielder for this coroutine. None while the coroutine is running or not yet started.
    yielder: Option<NonNull<Yielder<(), UnwindReason>>>,
    /// size of the associated stack. Required to return the stack to the pool.
    stack_size: usize,
}
static STACK_POOL: LazyLock<StackPool> = LazyLock::new(StackPool::new);

impl<T: 'static> Continuation<T> {
    fn new<F: FnOnce() -> T + 'static>(f: F, stack_size: usize) -> Self {
        // Allocating a new stack is pretty expensive since it involves several
        // system calls. We therefore keep a cache of pre-allocated stacks which
        // allows them to be reused multiple times.
        // FIXME(Amanieu): We should refactor this to avoid the lock.
        let stack = STACK_POOL.get_stack(stack_size);

        let coro = Coroutine::with_stack(stack, move |yielder, ()| {
            // Save the yielder to the global when resuming the function for the first time.
            // It will be stored to the continuation when yielding.
            YIELDER.with(|cell| cell.set(Some(yielder.into())));
            Ok(f())
        });
        Self {
            coro,
            yielder: None,
            stack_size,
        }
    }
    fn resume(
        mut self,
        trap_handler: Option<*const TrapHandlerFn<'static>>,
    ) -> (Option<Self>, Result<T, UnwindReason>) {
        // Set the global YIELDER to the current coroutine's yielder
        // If the coroutine is started for the first time, the yielder set here is None and it will get set during the first call to resume
        let previous_yielder = YIELDER.with(|cell| cell.replace(self.yielder));
        // Ensure that there is no previous YIELDER set. This would indicate that we are already on a Wasm stack.
        assert_eq!(previous_yielder, None);

        let mut own_yielder = None;

        let result = {
            defer! {
                // Previous yielder must be None here
                assert_eq!(previous_yielder, None);
                own_yielder = YIELDER.with(|cell| cell.replace(previous_yielder));
            }
            let coro = &mut self.coro;
            let coro_trap_handler = coro.trap_handler();

            TrapHandlerContext::install(trap_handler, coro_trap_handler, || {
                // let mut coro = cloned_coro.lock().unwrap();
                match coro.resume(()) {
                    CoroutineResult::Yield(trap) => {
                        // TODO: Figure out if it is safe to only do this in self.finish()
                        // // This came from unwind_with which requires that there be only
                        // // Wasm code on the stack.
                        // if !trap.is_resumable() {
                        //     // If the trap is not resumable, we need to reset the coroutine
                        //     // so that it can be used again.
                        //     unsafe {
                        //         // Cleanup
                        //         coro.force_reset();
                        //     }
                        // }
                        Err(trap)
                    }
                    CoroutineResult::Return(result) => result,
                }
            })
        };

        let is_resumable = match &result {
            Err(unwind_reason) => unwind_reason.is_resumable(),
            _ => false,
        };
        if is_resumable {
            // Own yielder must be set at this point
            assert_ne!(own_yielder, None);
            self.yielder = own_yielder;
            return (Some(self), result);
        }
        // TODO: Investigate if we need to defer this
        self.finish();
        (None, result)
    }
    /// Finish the continuation, returning its stack to the pool
    fn finish(self) {
        let mut coro = self.coro;
        if !coro.done() {
            unsafe {
                // Cleanup
                coro.force_reset();
            }
        }
        let stack = coro.into_stack();
        STACK_POOL.return_stack(stack, self.stack_size);
    }
}

/// A type-erased continuation that can be stored in a map.
struct ErasedContinuation {
    /// Owned pointer to the inner coroutine
    inner: *mut Continuation<()>,
}
impl<T: 'static> From<Continuation<T>> for ErasedContinuation {
    fn from(coro: Continuation<T>) -> Self {
        Self {
            inner: Box::into_raw(Box::new(coro)) as *mut Continuation<()>,
        }
    }
}
// TODO: This from implementation is horribly unsafe
impl<T: 'static> From<ErasedContinuation> for Continuation<T> {
    fn from(coro: ErasedContinuation) -> Self {
        let boxed_coro = unsafe { Box::from_raw(coro.inner as *mut Self) };
        *boxed_coro
    }
}

/// Runs the given function on a separate stack so that its stack usage can be
/// bounded. Stack overflows and other traps can be caught and execution
/// returned to the root of the stack.
fn on_wasm_stack<F: FnOnce() -> T + 'static, T: 'static>(
    stack_size: usize,
    trap_handler: Option<*const TrapHandlerFn<'static>>,
    f: F,
) -> Result<T, UnwindReason> {
    // TODO: Think about an optimization where we dont store and restore the continuation
    let continuation = ContinuationRef::new(f, stack_size);
    continuation.resume(trap_handler)
}

fn on_wasm_stack_resume<T: 'static>(
    continuation: u64,
    trap_handler: Option<*const TrapHandlerFn<'static>>,
) -> Result<T, UnwindReason> {
    let continuation_ref = ContinuationRef { id: continuation };
    continuation_ref.resume(trap_handler)
}

/// When executing on the Wasm stack, temporarily switch back to the host stack
/// to perform an operation that should not be constrainted by the Wasm stack
/// limits.
///
/// This is particularly important since the usage of the Wasm stack is under
/// the control of untrusted code. Malicious code could artificially induce a
/// stack overflow in the middle of a sensitive host operations (e.g. growing
/// a memory) which would be hard to recover from.
#[inline(always)]
pub fn on_host_stack<F: FnOnce() -> T, T>(f: F) -> T {
    // Reset YIELDER to None for the duration of this call to indicate that we
    // are no longer on the Wasm stack.
    let Some(yielder) = YIELDER.with(|cell| cell.replace(None)) else {
        // If we are already on the host stack, execute the function directly. This
        // happens if a host function is called directly from the API.
        return f();
    };

    // Restore YIELDER upon exiting normally or unwinding.
    defer! {
        YIELDER.with(|cell| cell.set(Some(yielder)));
    }

    // on_parent_stack requires the closure to be Send so that the Yielder
    // cannot be called from the parent stack. This is not a problem for us
    // since we don't expose the Yielder.
    struct SendWrapper<T>(T);
    unsafe impl<T> Send for SendWrapper<T> {}
    let wrapped = SendWrapper(f);
    unsafe { yielder.as_ref() }.on_parent_stack(move || {
        let wrapped = wrapped;
        (wrapped.0)()
    })
}

#[cfg(windows)]
pub fn lazy_per_thread_init() -> Result<(), Trap> {
    // We need additional space on the stack to handle stack overflow
    // exceptions. Rust's initialization code sets this to 0x5000 but this
    // seems to be insufficient in practice.
    use windows_sys::Win32::System::Threading::SetThreadStackGuarantee;
    if unsafe { SetThreadStackGuarantee(&mut 0x10000) } == 0 {
        panic!("failed to set thread stack guarantee");
    }

    Ok(())
}

/// A module for registering a custom alternate signal stack (sigaltstack).
///
/// Rust's libstd installs an alternate stack with size `SIGSTKSZ`, which is not
/// always large enough for our signal handling code. Override it by creating
/// and registering our own alternate stack that is large enough and has a guard
/// page.
#[cfg(unix)]
pub fn lazy_per_thread_init() -> Result<(), Trap> {
    use std::ptr::null_mut;

    thread_local! {
        /// Thread-local state is lazy-initialized on the first time it's used,
        /// and dropped when the thread exits.
        static TLS: Tls = unsafe { init_sigstack() };
    }

    /// The size of the sigaltstack (not including the guard, which will be
    /// added). Make this large enough to run our signal handlers.
    const MIN_STACK_SIZE: usize = 16 * 4096;

    enum Tls {
        OutOfMemory,
        Allocated {
            mmap_ptr: *mut libc::c_void,
            mmap_size: usize,
        },
        BigEnough,
    }

    unsafe fn init_sigstack() -> Tls {
        unsafe {
            // Check to see if the existing sigaltstack, if it exists, is big
            // enough. If so we don't need to allocate our own.
            let mut old_stack = mem::zeroed();
            let r = libc::sigaltstack(ptr::null(), &mut old_stack);
            assert_eq!(r, 0, "learning about sigaltstack failed");
            if old_stack.ss_flags & libc::SS_DISABLE == 0 && old_stack.ss_size >= MIN_STACK_SIZE {
                return Tls::BigEnough;
            }

            // ... but failing that we need to allocate our own, so do all that
            // here.
            let page_size: usize = region::page::size();
            let guard_size = page_size;
            let alloc_size = guard_size + MIN_STACK_SIZE;

            let ptr = libc::mmap(
                null_mut(),
                alloc_size,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            );
            if ptr == libc::MAP_FAILED {
                return Tls::OutOfMemory;
            }

            // Prepare the stack with readable/writable memory and then register it
            // with `sigaltstack`.
            let stack_ptr = (ptr as usize + guard_size) as *mut libc::c_void;
            let r = libc::mprotect(
                stack_ptr,
                MIN_STACK_SIZE,
                libc::PROT_READ | libc::PROT_WRITE,
            );
            assert_eq!(r, 0, "mprotect to configure memory for sigaltstack failed");
            let new_stack = libc::stack_t {
                ss_sp: stack_ptr,
                ss_flags: 0,
                ss_size: MIN_STACK_SIZE,
            };
            let r = libc::sigaltstack(&new_stack, ptr::null_mut());
            assert_eq!(r, 0, "registering new sigaltstack failed");

            Tls::Allocated {
                mmap_ptr: ptr,
                mmap_size: alloc_size,
            }
        }
    }

    // Ensure TLS runs its initializer and return an error if it failed to
    // set up a separate stack for signal handlers.
    return TLS.with(|tls| {
        if let Tls::OutOfMemory = tls {
            Err(Trap::oom())
        } else {
            Ok(())
        }
    });

    impl Drop for Tls {
        fn drop(&mut self) {
            let (ptr, size) = match self {
                Self::Allocated {
                    mmap_ptr,
                    mmap_size,
                } => (*mmap_ptr, *mmap_size),
                _ => return,
            };
            unsafe {
                // Deallocate the stack memory.
                let r = libc::munmap(ptr, size);
                debug_assert_eq!(r, 0, "munmap failed during thread shutdown");
            }
        }
    }
}
