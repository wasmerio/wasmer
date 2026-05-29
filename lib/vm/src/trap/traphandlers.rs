// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

#![allow(static_mut_refs)]

//! WebAssembly trap handling, which is built on top of the lower-level
//! signalhandling mechanisms.

#[cfg(all(unix, feature = "experimental-host-interrupt"))]
use crate::interrupt_registry;
use crate::vmcontext::{VMFunctionContext, VMTrampoline};
use crate::{Trap, VMContext, VMFunctionBody};
use backtrace::Backtrace;
use bytesize::ByteSize;
use core::ptr::{read, read_unaligned};
use corosensei::stack::{DefaultStack, Stack};
use corosensei::trap::{CoroutineTrapHandler, TrapHandlerRegs};
use corosensei::{CoroutineResult, ScopedCoroutine, Yielder};
use scopeguard::defer;
use std::any::Any;
use std::cell::Cell;
use std::error::Error;
use std::io;
use std::mem;
#[cfg(unix)]
use std::mem::MaybeUninit;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering, compiler_fence};
use std::sync::{LazyLock, Once};
use wasmer_types::TrapCode;

/// Convenience extension for [`Stack`] that exposes the total mapped size.
trait StackExt: Stack {
    /// Returns the total size of the stack mapping (including guard page).
    fn size(&self) -> usize {
        self.base().get() - self.limit().get()
    }
}
impl<T: Stack> StackExt for T {}

/// Configuration for the runtime VM
/// Currently only the stack size is configurable
pub struct VMConfig {
    /// Optional stack size (in byte) of the VM. Value lower than 8K will be rounded to 8K.
    pub wasm_stack_size: Option<usize>,
}

// TrapInformation can be stored in the "Undefined Instruction" itself.
// On x86_64, 0xC? select a "Register" for the Mod R/M part of "ud1" (so with no other bytes after)
// On Arm64, the udf allows for a 16bits values, so we'll use the same 0xC? to store the trapinfo
static MAGIC: u8 = 0xc0;

static DEFAULT_STACK_SIZE: AtomicUsize = AtomicUsize::new(ByteSize::mib(1).as_u64() as usize);

/// Maximum allowed default stack size (100 MiB) for the process-wide
/// configuration set via `set_stack_size`.
pub const MAX_STACK_SIZE: usize = ByteSize::mib(100).as_u64() as usize;

// Current definition of `ucontext_t` in the `libc` crate is incorrect
// on aarch64-apple-drawing so it's defined here with a more accurate definition.
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

#[cfg(all(unix, not(all(target_arch = "aarch64", target_os = "macos"))))]
use libc::ucontext_t;

/// Sets the process-wide default stack size for new Wasmer coroutines.
/// The value is clamped to [8 KiB, MAX_STACK_SIZE].
pub fn set_stack_size(size: usize) {
    DEFAULT_STACK_SIZE.store(
        size.clamp(ByteSize::kib(8).as_u64() as usize, MAX_STACK_SIZE),
        Ordering::Relaxed,
    );
}

/// Returns the process-wide default stack size in bytes.
pub fn get_stack_size() -> usize {
    DEFAULT_STACK_SIZE.load(Ordering::Relaxed)
}

/// Pool of pre-allocated coroutine stacks to avoid repeated mmap syscalls.
/// Acts as the cross-thread overflow store; per-thread reuse is served by
/// `TLS_STACK` to keep the hot path atomic-free.
static STACK_POOL: LazyLock<crossbeam_queue::SegQueue<DefaultStack>> =
    LazyLock::new(crossbeam_queue::SegQueue::new);

/// Per-thread cache holding a single ready-to-use coroutine stack. The hot
/// path of `on_wasm_stack` pops from here without touching the global
/// `STACK_POOL`'s atomics; only the first call on a thread or re-entrant
/// nested calls fall back to the pool.
///
/// On thread exit the held stack (if any) is pushed to `STACK_POOL` so memory
/// cycles correctly across thread lifetimes (no mmap leaks).
struct StackCache(Cell<Option<DefaultStack>>);

impl Drop for StackCache {
    fn drop(&mut self) {
        if let Some(stack) = self.0.take() {
            STACK_POOL.push(stack);
        }
    }
}

thread_local! {
    static TLS_STACK: StackCache = const { StackCache(Cell::new(None)) };
}

/// Acquire a coroutine stack large enough for `min_size`. Prefers the
/// thread-local cache (no atomics), falls back to the global `STACK_POOL`,
/// then allocates a fresh stack.
fn acquire_stack(min_size: usize) -> DefaultStack {
    // Fast path: thread-local cache. Steady-state per-thread reuse never
    // touches the SegQueue.
    if let Some(stack) = TLS_STACK.with(|cache| cache.0.take()) {
        if stack.size() >= min_size {
            return stack;
        }
        // Undersized — discard (mirrors the existing `STACK_POOL.pop().filter(...)`
        // behavior of not holding undersized stacks in rotation).
        drop(stack);
    }
    // Cross-thread overflow pool. Single pop, single filter — same semantics
    // as the pre-TLS implementation.
    STACK_POOL
        .pop()
        .filter(|s| s.size() >= min_size)
        .unwrap_or_else(|| DefaultStack::new(min_size).unwrap())
}

/// Release a coroutine stack. Prefers the thread-local slot if empty (no
/// atomics); otherwise pushes to the global `STACK_POOL` so the stack is
/// still reusable by other threads.
fn release_stack(stack: DefaultStack) {
    let displaced = TLS_STACK.with(|cache| cache.0.replace(Some(stack)));
    if let Some(displaced) = displaced {
        STACK_POOL.push(displaced);
    }
}

/// Drains the coroutine stack pool at the moment it runs.
///
/// This is intended to be called before retrying with a larger stack size so
/// that the pool does not keep serving cached undersized stacks.
///
/// Note that `STACK_POOL` is a global, concurrently used queue and that each
/// thread also keeps a private cached stack in TLS. Other threads may push
/// stacks back into the pool (for example, when their Wasm execution
/// finishes) while or after this function is running, and TLS-cached stacks
/// on other threads are not touched. As a result, this function provides
/// only a best-effort drain: there is no guarantee that no undersized stacks
/// exist immediately after it returns unless the caller ensures, via external
/// synchronization, that no other Wasm executions can return stacks to the
/// pool while this function runs. The current thread's TLS-cached stack is
/// drained as part of this call.
pub fn drain_stack_pool() {
    // Drain the calling thread's TLS slot first (best-effort across threads
    // still applies — other threads' caches aren't touched).
    if let Some(stack) = TLS_STACK.with(|cache| cache.0.take()) {
        drop(stack);
    }
    while STACK_POOL.pop().is_some() {}
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
    let mut val: Option<u8> = None;
    unsafe {
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
        if cfg!(target_arch = "riscv64") {
            let addr = addr as *mut u32;
            // Check if 'unimp' instruction
            val = if read(addr) == 0xc0001073 {
                // Read from the instruction we emitted: 'addi a0, xzero, $payload'
                // and take the encoded immediate value (upper 12-bits).
                let prev_insn = read(addr.sub(1));
                if (prev_insn & 0xffff) == 0x0513 {
                    Some((prev_insn >> 20) as u8)
                } else {
                    None
                }
            } else {
                None
            };
        }
    }

    // The direct encoding of a trap into the instruction is unused on RISC-V:
    if cfg!(target_arch = "x86_64") || cfg!(target_arch = "aarch64") {
        val = val.and_then(|val| {
            if val & MAGIC == MAGIC {
                Some(val & 0xf)
            } else {
                None
            }
        });
    }

    match val {
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

cfg_if::cfg_if! {
    if #[cfg(unix)] {
        static mut PREV_SIGSEGV: MaybeUninit<libc::sigaction> = MaybeUninit::uninit();
        static mut PREV_SIGBUS: MaybeUninit<libc::sigaction> = MaybeUninit::uninit();
        static mut PREV_SIGILL: MaybeUninit<libc::sigaction> = MaybeUninit::uninit();
        static mut PREV_SIGFPE: MaybeUninit<libc::sigaction> = MaybeUninit::uninit();

        #[cfg(feature = "experimental-host-interrupt")]
        static mut PREV_SIGUSR1: MaybeUninit<libc::sigaction> = MaybeUninit::uninit();

        unsafe fn platform_init() { unsafe {
            let register = |slot: &mut MaybeUninit<libc::sigaction>, signal: i32, nodefer: bool| {
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
                handler.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
                if nodefer {
                    handler.sa_flags |= libc::SA_NODEFER;
                }
                handler.sa_sigaction = trap_handler as *const () as usize;
                libc::sigemptyset(&mut handler.sa_mask);
                if libc::sigaction(signal, &handler, slot.as_mut_ptr()) != 0 {
                    panic!(
                        "unable to install signal handler: {}",
                        io::Error::last_os_error(),
                    );
                }
            };

            // Allow handling OOB with signals on all architectures
            register(&mut PREV_SIGSEGV, libc::SIGSEGV, true);

            // Handle `unreachable` instructions which execute `ud2` right now
            register(&mut PREV_SIGILL, libc::SIGILL, true);

            // SIGUSR1 is used to interrupt long-running WASM code.
            // It doesn't use NODEFER since, if a second interruption
            // request comes in while one is already being processed,
            // there's nothing meaningful we can do.
            #[cfg(feature = "experimental-host-interrupt")]
            register(&mut PREV_SIGUSR1, libc::SIGUSR1, false);

            // x86 uses SIGFPE to report division by zero
            if cfg!(target_arch = "x86") || cfg!(target_arch = "x86_64") {
                register(&mut PREV_SIGFPE, libc::SIGFPE, true);
            }

            // On ARM, handle Unaligned Accesses.
            // On Darwin, guard page accesses are raised as SIGBUS.
            if cfg!(target_arch = "arm") || cfg!(target_vendor = "apple") {
                register(&mut PREV_SIGBUS, libc::SIGBUS, true);
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
                #[cfg(feature = "experimental-host-interrupt")]
                libc::SIGUSR1 => &PREV_SIGUSR1,
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
                #[cfg(feature = "experimental-host-interrupt")]
                libc::SIGUSR1 => {
                    // If we're not running WASM code from the specific store for which
                    // an interrupt was requested, there's nothing to do.
                    if !interrupt_registry::on_interrupted() {
                        return;
                    }
                    Some(TrapCode::HostInterrupt)
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

            // If we're not running WASM code at all, there's nothing to
            // do for an interrupt.
            #[cfg(feature = "experimental-host-interrupt")]
            if signum == libc::SIGUSR1 {
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
                } else if #[cfg(all(target_os = "linux", target_arch = "powerpc64"))] {
                    pc = (*context.uc_mcontext.regs).nip as usize;
                    sp = (*context.uc_mcontext.regs).gpr[1] as usize;
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
                    context.uc_mcontext.mc_gpregs.gp_lr = lr as libc::register_t;
                } else if #[cfg(all(target_os = "linux", target_arch = "loongarch64"))] {
                    let TrapHandlerRegs { pc, sp, a0, a1, fp, ra } = regs;
                    context.uc_mcontext.__pc = pc;
                    context.uc_mcontext.__gregs[1] = ra;
                    context.uc_mcontext.__gregs[3] = sp;
                    context.uc_mcontext.__gregs[4] = a0;
                    context.uc_mcontext.__gregs[5] = a1;
                    context.uc_mcontext.__gregs[22] = fp;
                } else if #[cfg(all(target_os = "linux", target_arch = "powerpc64"))] {
                    let TrapHandlerRegs { pc, sp, r3, r4, r31, lr } = regs;
                    (*context.uc_mcontext.regs).nip = pc;
                    (*context.uc_mcontext.regs).gpr[1] = sp;
                    (*context.uc_mcontext.regs).gpr[3] = r3;
                    (*context.uc_mcontext.regs).gpr[4] = r4;
                    (*context.uc_mcontext.regs).gpr[31] = r31;
                    (*context.uc_mcontext.regs).link = lr;
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
                    pc = context.Eip as usize;
                    sp = context.Esp as usize;
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
pub unsafe fn raise_lib_trap(trap: Trap) -> ! {
    unsafe { unwind_with(UnwindReason::LibTrap(trap)) }
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
            if let Some(trap_handler) = ctx.custom_trap
                && call_handler(&*trap_handler)
            {
                return true;
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
            // happening during a function prologue, which can lead the unwinder to
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

enum UnwindReason {
    /// A panic caused by the host
    Panic(Box<dyn Any + Send>),
    /// A custom error triggered by the user
    UserTrap(Box<dyn Error + Send + Sync>),
    /// A Trap triggered by a wasm libcall
    LibTrap(Trap),
    /// A trap caused by the Wasm generated code
    WasmTrap {
        backtrace: Backtrace,
        pc: usize,
        signal_trap: Option<TrapCode>,
    },
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
}

unsafe fn unwind_with(reason: UnwindReason) -> ! {
    unsafe {
        let yielder = YIELDER
            .with(|cell| cell.replace(None))
            .expect("not running on Wasm stack");

        yielder.as_ref().suspend(reason);

        // on_wasm_stack will forcibly reset the coroutine stack after yielding.
        unreachable!();
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
    // Reuse a cached stack — TLS first (atomic-free hot path), then the
    // cross-thread overflow pool, then allocate fresh. Size mismatches
    // (e.g. after `drain_stack_pool()` + a stack-size change) are filtered
    // inside `acquire_stack`. `base() - limit()` is the full mmap region
    // (including guard page), which is always >= the requested size for
    // stacks allocated with that size.
    let stack = acquire_stack(stack_size);
    let mut stack = scopeguard::guard(stack, release_stack);

    // Create a coroutine with a new stack to run the function on.
    let coro = ScopedCoroutine::with_stack(&mut *stack, move |yielder, ()| {
        // Save the yielder to TLS so that it can be used later.
        YIELDER.with(|cell| cell.set(Some(yielder.into())));

        Ok(f())
    });

    // Ensure that YIELDER is reset on exit even if the coroutine panics,
    defer! {
        YIELDER.with(|cell| cell.set(None));
    }

    coro.scope(|mut coro_ref| {
        // Set up metadata for the trap handler for the duration of the coroutine
        // execution. This is restored to its previous value afterwards.
        TrapHandlerContext::install(trap_handler, coro_ref.trap_handler(), || {
            match coro_ref.resume(()) {
                CoroutineResult::Yield(trap) => {
                    // This came from unwind_with which requires that there be only
                    // Wasm code on the stack.
                    unsafe {
                        coro_ref.force_reset();
                    }
                    Err(trap)
                }
                CoroutineResult::Return(result) => result,
            }
        })
    })
}

/// When executing on the Wasm stack, temporarily switch back to the host stack
/// to perform an operation that should not be constrained by the Wasm stack
/// limits.
///
/// This is particularly important since the usage of the Wasm stack is under
/// the control of untrusted code. Malicious code could artificially induce a
/// stack overflow in the middle of a sensitive host operations (e.g. growing
/// a memory) which would be hard to recover from.
pub fn on_host_stack<F: FnOnce() -> T, T>(f: F) -> T {
    // Reset YIEDER to None for the duration of this call to indicate that we
    // are no longer on the Wasm stack.
    let yielder_ptr = YIELDER.with(|cell| cell.replace(None));

    // If we are already on the host stack, execute the function directly. This
    // happens if a host function is called directly from the API.
    let yielder = match yielder_ptr {
        Some(ptr) => unsafe { ptr.as_ref() },
        None => return f(),
    };

    // Restore YIELDER upon exiting normally or unwinding.
    defer! {
        YIELDER.with(|cell| cell.set(yielder_ptr));
    }

    // on_parent_stack requires the closure to be Send so that the Yielder
    // cannot be called from the parent stack. This is not a problem for us
    // since we don't expose the Yielder.
    struct SendWrapper<T>(T);
    unsafe impl<T> Send for SendWrapper<T> {}
    let wrapped = SendWrapper(f);
    yielder.on_parent_stack(move || {
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
    const MIN_STACK_SIZE: usize = ByteSize::kib(64).as_u64() as usize;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Guards tests that mutate global state (DEFAULT_STACK_SIZE, STACK_POOL).
    // Rust runs tests in parallel by default; this mutex serializes them so
    // they don't step on each other.
    static GLOBAL_STATE: Mutex<()> = Mutex::new(());

    /// Saves the current stack size and restores it on drop (even on panic).
    struct RestoreStackSize(usize);
    impl Drop for RestoreStackSize {
        fn drop(&mut self) {
            set_stack_size(self.0);
        }
    }

    #[test]
    fn max_stack_size_is_100mb() {
        assert_eq!(MAX_STACK_SIZE, ByteSize::mib(100).as_u64() as usize);
    }

    #[test]
    fn get_set_stack_size_roundtrip() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        let new_size = ByteSize::mib(4).as_u64() as usize;
        set_stack_size(new_size);
        assert_eq!(get_stack_size(), new_size);
    }

    #[test]
    fn set_stack_size_clamps_to_min() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        set_stack_size(1); // way below 8 KiB minimum
        assert_eq!(get_stack_size(), ByteSize::kib(8).as_u64() as usize);
    }

    #[test]
    fn set_stack_size_clamps_to_max() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        set_stack_size(usize::MAX);
        assert_eq!(get_stack_size(), MAX_STACK_SIZE);
    }

    #[test]
    fn drain_stack_pool_empties_pool() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let stack = DefaultStack::new(ByteSize::mib(1).as_u64() as usize).unwrap();
        STACK_POOL.push(stack);
        assert!(!STACK_POOL.is_empty());
        drain_stack_pool();
        assert!(STACK_POOL.is_empty());
    }

    #[test]
    fn drain_stack_pool_is_idempotent() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        drain_stack_pool();
        drain_stack_pool(); // second call on empty pool should not panic
        assert!(STACK_POOL.is_empty());
    }

    /// The stack pool is not size-aware, so after a stack size increase it keeps
    /// serving cached undersized stacks. `drain_stack_pool()` breaks the cycle.
    ///
    /// 1. A call fills the pool with 500 KiB stacks (simulating normal execution).
    /// 2. The caller doubles the default to 1 MiB (simulating overflow retry).
    /// 3. WITHOUT draining, the pool still hands back a 500 KiB stack — the
    ///    retry would overflow again, creating an infinite loop.
    /// 4. After `drain_stack_pool()`, the pool is empty and the next allocation
    ///    must use the new, larger size.
    #[test]
    fn pool_returns_stale_stack_without_drain() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();

        // --- Phase 1: simulate normal execution that returns a 500 KiB stack ---
        let small_size = ByteSize::kib(500).as_u64() as usize;
        let small_stack = DefaultStack::new(small_size).unwrap();
        STACK_POOL.push(small_stack);

        // --- Phase 2: "overflow detected" — caller doubles the default ---
        let big_size = ByteSize::mib(1).as_u64() as usize;
        set_stack_size(big_size);
        assert_eq!(get_stack_size(), big_size);

        // --- Phase 3: WITHOUT drain, pool still returns the old small stack ---
        // This is the bug: the caller asked for a bigger stack but the pool
        // serves a cached undersized one, causing the retry to overflow again.
        let stale = STACK_POOL.pop();
        assert!(
            stale.is_some(),
            "pool should still contain the old stack (the bug scenario)"
        );

        // --- Phase 4: with drain, pool is empty — next alloc uses new size ---
        STACK_POOL.push(stale.unwrap());
        drain_stack_pool();
        assert!(
            STACK_POOL.pop().is_none(),
            "after drain, pool must be empty so a fresh stack is allocated at the new size"
        );
    }

    /// `on_wasm_stack` discards undersized stacks from the pool and allocates
    /// a fresh one instead of blindly reusing whatever the pool returns.
    #[test]
    fn on_wasm_stack_discards_undersized_stack() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();

        // Push an undersized stack into the pool.
        let small_size = ByteSize::kib(500).as_u64() as usize;
        let small_stack = DefaultStack::new(small_size).unwrap();
        STACK_POOL.push(small_stack);

        // Request a larger stack via on_wasm_stack.
        let big_size = ByteSize::mib(1).as_u64() as usize;
        let result = on_wasm_stack(big_size, None, || 42);

        assert_eq!(result.ok().expect("on_wasm_stack should succeed"), 42);
        // The undersized stack was discarded; the correctly-sized stack
        // allocated for the call now lives in the TLS cache (the hot path).
        // It will end up in the global pool only on thread exit or eviction.
        let returned = TLS_STACK
            .with(|cache| cache.0.take())
            .or_else(|| STACK_POOL.pop())
            .expect("stack should have been returned to TLS cache or pool");
        assert!(
            returned.size() >= big_size,
            "returned stack must be at least as large as the requested size"
        );
    }

    /// After a wasm call, the freshly-used stack stays in the thread-local
    /// cache so subsequent calls on the same thread reuse it without touching
    /// the global SegQueue.
    #[test]
    fn tls_stack_caches_after_first_call() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();

        let size = get_stack_size();

        // First call: TLS + pool both empty → allocate fresh; stack ends in TLS.
        assert!(on_wasm_stack(size, None, || ()).is_ok());
        assert!(STACK_POOL.is_empty(), "pool should still be empty after a TLS-served call");

        // Verify TLS holds a stack, then put it back.
        let cached_present = TLS_STACK.with(|cache| {
            let taken = cache.0.take();
            let present = taken.is_some();
            cache.0.set(taken);
            present
        });
        assert!(cached_present, "TLS slot should hold the post-call stack");

        // Second call should consume from TLS; pool stays empty.
        assert!(on_wasm_stack(size, None, || ()).is_ok());
        assert!(STACK_POOL.is_empty(), "second call must not push to the global pool");
        let still_cached = TLS_STACK.with(|cache| {
            let taken = cache.0.take();
            let present = taken.is_some();
            cache.0.set(taken);
            present
        });
        assert!(still_cached, "TLS slot should still hold a stack after the second call");

        // Cleanup: clear TLS so we don't leak into other tests.
        TLS_STACK.with(|cache| cache.0.set(None));
    }

    /// On thread exit, the TLS cache's `Drop` impl returns the held stack to
    /// the global pool so memory cycles correctly across thread lifetimes.
    #[test]
    fn tls_stack_returns_to_pool_on_thread_exit() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();

        let size = get_stack_size();

        let handle = std::thread::spawn(move || {
            assert!(on_wasm_stack(size, None, || ()).is_ok());
        });
        handle.join().unwrap();

        // The spawned thread's TLS cache was dropped on join; the stack must
        // have made it back to the global pool.
        let returned = STACK_POOL
            .pop()
            .expect("thread exit should return TLS-cached stack to the global pool");
        assert!(returned.size() >= size);
    }

    // -----------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------

    /// Clears the current thread's TLS slot so tests don't see state from
    /// previous tests (they share the same thread under cargo test's
    /// per-test serialization via `GLOBAL_STATE`).
    fn clear_tls_stack() {
        TLS_STACK.with(|cache| cache.0.set(None));
    }

    /// `base().get() - limit().get()` (i.e. `Stack::size`) is constant per
    /// `DefaultStack` instance, but `base().get()` itself uniquely identifies
    /// the mmap allocation. We use it as a cheap identity check to see which
    /// stack was returned by acquire/release.
    fn stack_id(stack: &DefaultStack) -> usize {
        stack.base().get()
    }

    // -----------------------------------------------------------------
    // acquire_stack mechanics
    // -----------------------------------------------------------------

    #[test]
    fn acquire_allocates_fresh_when_tls_and_pool_empty() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let size = get_stack_size();
        let stack = acquire_stack(size);
        assert!(stack.size() >= size, "freshly allocated stack must satisfy min_size");

        drop(stack);
        clear_tls_stack();
        drain_stack_pool();
    }

    #[test]
    fn acquire_prefers_tls_over_pool() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let size = get_stack_size();
        let tls_stack = DefaultStack::new(size).unwrap();
        let tls_id = stack_id(&tls_stack);
        TLS_STACK.with(|cache| cache.0.set(Some(tls_stack)));

        let pool_stack = DefaultStack::new(size).unwrap();
        let pool_id = stack_id(&pool_stack);
        STACK_POOL.push(pool_stack);

        let got = acquire_stack(size);
        assert_eq!(stack_id(&got), tls_id, "acquire must prefer TLS over pool");
        assert_ne!(stack_id(&got), pool_id);

        drop(got);
        clear_tls_stack();
        drain_stack_pool();
    }

    #[test]
    fn acquire_uses_pool_when_tls_empty() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let size = get_stack_size();
        let pool_stack = DefaultStack::new(size).unwrap();
        let pool_id = stack_id(&pool_stack);
        STACK_POOL.push(pool_stack);

        let got = acquire_stack(size);
        assert_eq!(stack_id(&got), pool_id, "acquire must consume from pool when TLS is empty");
        assert!(STACK_POOL.is_empty(), "pool stack must be removed when used");

        drop(got);
        clear_tls_stack();
        drain_stack_pool();
    }

    #[test]
    fn acquire_discards_undersized_tls_then_allocates() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let small_size = ByteSize::kib(512).as_u64() as usize;
        let undersized = DefaultStack::new(small_size).unwrap();
        TLS_STACK.with(|cache| cache.0.set(Some(undersized)));

        let big_size = ByteSize::mib(2).as_u64() as usize;
        let got = acquire_stack(big_size);

        // The acquired stack must be at least the requested size. We do NOT
        // compare base addresses: the OS can reuse a freshly munmap'd
        // virtual address for the next mmap, so pointer identity is not a
        // reliable "is this a different stack" check across a drop+alloc.
        // The meaningful semantic is that the undersized stack was taken
        // out of rotation (TLS empty, not silently pushed to the pool) and
        // the returned stack is sized correctly.
        assert!(got.size() >= big_size, "acquired stack must satisfy big_size");
        let tls_empty = TLS_STACK.with(|cache| {
            let s = cache.0.take();
            let empty = s.is_none();
            cache.0.set(s);
            empty
        });
        assert!(tls_empty, "undersized TLS stack must have been taken and discarded");
        assert!(
            STACK_POOL.is_empty(),
            "undersized TLS stack must be discarded, not pushed to the pool",
        );

        drop(got);
        clear_tls_stack();
        drain_stack_pool();
    }

    #[test]
    fn acquire_discards_undersized_pool_then_allocates() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let small_size = ByteSize::kib(512).as_u64() as usize;
        let undersized = DefaultStack::new(small_size).unwrap();
        STACK_POOL.push(undersized);

        let big_size = ByteSize::mib(2).as_u64() as usize;
        let got = acquire_stack(big_size);

        // Same caveat as the TLS variant: mmap may reuse the virtual
        // address of the dropped undersized stack for the new big stack,
        // so we verify the semantic outcome — the pool was drained of the
        // undersized entry and the returned stack is sized correctly.
        assert!(got.size() >= big_size, "acquired stack must satisfy big_size");
        assert!(
            STACK_POOL.is_empty(),
            "undersized pool stack must have been popped, filtered out and dropped",
        );

        drop(got);
        clear_tls_stack();
        drain_stack_pool();
    }

    // -----------------------------------------------------------------
    // release_stack mechanics
    // -----------------------------------------------------------------

    #[test]
    fn release_into_empty_tls_caches_there() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        drain_stack_pool();
        clear_tls_stack();

        let size = get_stack_size();
        let stack = DefaultStack::new(size).unwrap();
        let id = stack_id(&stack);
        release_stack(stack);

        let in_tls = TLS_STACK
            .with(|cache| cache.0.take())
            .expect("release into empty TLS should leave the stack in TLS");
        assert_eq!(stack_id(&in_tls), id);
        assert!(STACK_POOL.is_empty(), "pool must not be touched when TLS is empty");

        drain_stack_pool();
    }

    #[test]
    fn release_into_occupied_tls_displaces_older_to_pool() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        drain_stack_pool();
        clear_tls_stack();

        let size = get_stack_size();
        let older = DefaultStack::new(size).unwrap();
        let older_id = stack_id(&older);
        TLS_STACK.with(|cache| cache.0.set(Some(older)));

        let newer = DefaultStack::new(size).unwrap();
        let newer_id = stack_id(&newer);
        release_stack(newer);

        let in_tls = TLS_STACK
            .with(|cache| cache.0.take())
            .expect("TLS should hold the newly-released stack");
        assert_eq!(stack_id(&in_tls), newer_id, "newer stack must displace into TLS");

        let displaced = STACK_POOL
            .pop()
            .expect("older stack should have been pushed to global pool");
        assert_eq!(stack_id(&displaced), older_id, "displaced stack must be the older one");

        drain_stack_pool();
    }

    // -----------------------------------------------------------------
    // drain_stack_pool extended semantics
    // -----------------------------------------------------------------

    #[test]
    fn drain_stack_pool_clears_calling_thread_tls_slot() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        drain_stack_pool();
        clear_tls_stack();

        let stack = DefaultStack::new(get_stack_size()).unwrap();
        TLS_STACK.with(|cache| cache.0.set(Some(stack)));

        drain_stack_pool();

        let tls_empty = TLS_STACK.with(|cache| cache.0.take().is_none());
        assert!(tls_empty, "drain_stack_pool must also clear current thread's TLS slot");
        assert!(STACK_POOL.is_empty());
    }

    // -----------------------------------------------------------------
    // on_wasm_stack functional behavior
    // -----------------------------------------------------------------

    #[test]
    fn on_wasm_stack_passes_closure_value_back() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let r = on_wasm_stack(get_stack_size(), None, || 12345u32);
        assert_eq!(r.ok(), Some(12345));

        clear_tls_stack();
        drain_stack_pool();
    }

    #[test]
    fn on_wasm_stack_passes_owning_result_back() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        // Use a heap-allocated value to verify the move-out path: the
        // closure produces a `Vec<u8>` that must travel from the coroutine
        // stack back to the host.
        let r = on_wasm_stack(get_stack_size(), None, || vec![0u8, 1, 2, 3, 4]);
        assert_eq!(r.ok(), Some(vec![0u8, 1, 2, 3, 4]));

        clear_tls_stack();
        drain_stack_pool();
    }

    #[test]
    fn many_calls_do_not_grow_global_pool() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        // With TLS caching, repeated single-threaded calls must keep
        // reusing the same TLS stack and never push to the global pool.
        for _ in 0..1000 {
            assert!(on_wasm_stack(get_stack_size(), None, || ()).is_ok());
        }
        assert!(
            STACK_POOL.is_empty(),
            "1000 sequential calls should not grow the global pool (TLS handles reuse)"
        );

        clear_tls_stack();
        drain_stack_pool();
    }

    // -----------------------------------------------------------------
    // Trap and unwind paths
    // -----------------------------------------------------------------

    #[test]
    fn raise_user_trap_yields_err() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let r: Result<(), UnwindReason> = on_wasm_stack(get_stack_size(), None, || {
            unsafe {
                raise_user_trap(Box::new(io::Error::other("user trap from test")));
            }
        });
        assert!(r.is_err(), "raise_user_trap must produce Err");

        clear_tls_stack();
        drain_stack_pool();
    }

    #[test]
    fn raise_lib_trap_yields_err() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let r: Result<(), UnwindReason> = on_wasm_stack(get_stack_size(), None, || {
            unsafe {
                raise_lib_trap(Trap::lib(TrapCode::IntegerDivisionByZero));
            }
        });
        assert!(r.is_err(), "raise_lib_trap must produce Err");

        clear_tls_stack();
        drain_stack_pool();
    }

    #[test]
    fn resume_panic_yields_err_without_unwinding() {
        // `resume_panic` packages the payload as `UnwindReason::Panic`. The
        // host-side panic resumption lives in `UnwindReason::into_trap`,
        // which we do NOT call here — `on_wasm_stack` itself just returns
        // the Err, so the test does not actually panic.
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let r: Result<(), UnwindReason> = on_wasm_stack(get_stack_size(), None, || {
            unsafe {
                resume_panic(Box::new("panic payload from test"));
            }
        });
        assert!(r.is_err(), "resume_panic must surface as Err to on_wasm_stack");

        clear_tls_stack();
        drain_stack_pool();
    }

    #[test]
    fn trap_does_not_corrupt_subsequent_calls() {
        // After a trap, the per-call coroutine is force-reset and dropped.
        // The TLS stack cache and global pool must remain in a usable state
        // so that subsequent calls succeed.
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let trapped: Result<(), UnwindReason> = on_wasm_stack(get_stack_size(), None, || {
            unsafe {
                raise_user_trap(Box::new(io::Error::other("first call traps")));
            }
        });
        assert!(trapped.is_err());

        // Subsequent normal call must still succeed.
        let ok = on_wasm_stack(get_stack_size(), None, || 7u32);
        assert_eq!(ok.ok(), Some(7), "calls after a trap must still work");

        clear_tls_stack();
        drain_stack_pool();
    }

    // -----------------------------------------------------------------
    // on_host_stack
    // -----------------------------------------------------------------

    #[test]
    fn on_host_stack_outside_coroutine_runs_inline() {
        // `on_host_stack` outside any wasm coroutine just runs `f()` directly
        // (no stack switch); this asserts the no-yielder branch still works.
        let _lock = GLOBAL_STATE.lock().unwrap();
        let n = on_host_stack(|| 99i32);
        assert_eq!(n, 99);
    }

    #[test]
    fn on_host_stack_inside_wasm_switches_and_returns() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let r = on_wasm_stack(get_stack_size(), None, || on_host_stack(|| 88i32));
        assert_eq!(r.ok(), Some(88));

        clear_tls_stack();
        drain_stack_pool();
    }

    // -----------------------------------------------------------------
    // Re-entrancy
    // -----------------------------------------------------------------

    #[test]
    fn reentrant_call_returns_value() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let outer = on_wasm_stack(get_stack_size(), None, || {
            on_wasm_stack(get_stack_size(), None, || 42i32)
                .ok()
                .expect("inner must succeed")
        });
        assert_eq!(outer.ok(), Some(42));

        clear_tls_stack();
        drain_stack_pool();
    }

    #[test]
    fn reentrant_calls_run_to_completion_under_pool_pressure() {
        // The outer call's stack is held by its scopeguard for the duration
        // of the call; the inner call must therefore pop a separate stack
        // from the pool (or allocate one). With a pre-populated pool the
        // inner call should consume that stack; either way the nested call
        // chain must complete without deadlocking or panicking from
        // corosensei.
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering as O};

        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        // Pre-populate the pool with one stack so the inner call can grab
        // it instead of allocating.
        let pre = DefaultStack::new(get_stack_size()).unwrap();
        STACK_POOL.push(pre);

        let inner_completed = Arc::new(AtomicUsize::new(0));
        let inner_completed_outer = inner_completed.clone();
        let _ = on_wasm_stack(get_stack_size(), None, move || {
            let inner_completed = inner_completed_outer.clone();
            let inner = on_wasm_stack(get_stack_size(), None, move || {
                inner_completed.fetch_add(1, O::Relaxed);
            });
            assert!(inner.is_ok(), "inner re-entrant call must succeed");
        });
        assert_eq!(
            inner_completed.load(O::Relaxed),
            1,
            "inner closure must have executed exactly once",
        );

        clear_tls_stack();
        drain_stack_pool();
    }

    #[test]
    fn reentrant_inner_trap_does_not_kill_outer() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let outer = on_wasm_stack(get_stack_size(), None, || {
            let inner: Result<i32, UnwindReason> = on_wasm_stack(get_stack_size(), None, || {
                unsafe {
                    raise_user_trap(Box::new(io::Error::other("inner trap")));
                }
            });
            // Outer observes inner's Err and recovers.
            match inner {
                Err(_) => 1234i32,
                Ok(_) => panic!("inner should have trapped"),
            }
        });
        assert_eq!(
            outer.ok(),
            Some(1234),
            "outer must recover after inner trap and run to completion"
        );

        clear_tls_stack();
        drain_stack_pool();
    }

    #[test]
    fn reentrant_with_on_host_stack_in_between() {
        // Outer wasm → on_host_stack → inner wasm. This exercises the
        // YIELDER save/restore in `unwind_with` / `on_host_stack` against
        // a re-entrant boundary.
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let r = on_wasm_stack(get_stack_size(), None, || {
            on_host_stack(|| {
                on_wasm_stack(get_stack_size(), None, || 5i32)
                    .ok()
                    .expect("nested inner must succeed")
            })
        });
        assert_eq!(r.ok(), Some(5));

        clear_tls_stack();
        drain_stack_pool();
    }

    // -----------------------------------------------------------------
    // Concurrency
    // -----------------------------------------------------------------

    #[test]
    fn many_threads_in_parallel_all_succeed() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();

        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering as O};

        let counter = Arc::new(AtomicUsize::new(0));
        const THREADS: usize = 8;
        const CALLS_PER_THREAD: usize = 200;

        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let counter = counter.clone();
                std::thread::spawn(move || {
                    let size = get_stack_size();
                    for _ in 0..CALLS_PER_THREAD {
                        if on_wasm_stack(size, None, || 1u32).ok() == Some(1) {
                            counter.fetch_add(1, O::Relaxed);
                        }
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(counter.load(O::Relaxed), THREADS * CALLS_PER_THREAD);
        // Pool should now hold at most `THREADS` stacks (one per thread that
        // exited). Each thread also drops its TLS slot on exit, which pushes
        // the stack to the pool.
        let mut pooled = 0usize;
        while STACK_POOL.pop().is_some() {
            pooled += 1;
        }
        assert!(
            pooled <= THREADS,
            "pool should hold at most one stack per terminated thread (got {pooled} for {THREADS} threads)"
        );

        clear_tls_stack();
        drain_stack_pool();
    }

    // -----------------------------------------------------------------
    // Stack size dynamics
    // -----------------------------------------------------------------

    #[test]
    fn growing_request_discards_smaller_tls_stack() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        // First call at a small size populates TLS with a small stack.
        let small = ByteSize::mib(1).as_u64() as usize;
        set_stack_size(small);
        assert!(on_wasm_stack(small, None, || ()).is_ok());

        let cached_size = TLS_STACK
            .with(|cache| {
                let s = cache.0.take();
                let sz = s.as_ref().map(|s| s.size()).unwrap_or(0);
                cache.0.set(s);
                sz
            });
        assert!(cached_size >= small, "TLS should hold the small-sized stack");

        // Now request a larger stack. acquire_stack should discard the TLS
        // entry and either pop a big-enough one from pool or allocate.
        let big = ByteSize::mib(4).as_u64() as usize;
        set_stack_size(big);
        assert!(on_wasm_stack(big, None, || ()).is_ok());

        // The TLS slot should now hold a stack that's big enough.
        let cached_size = TLS_STACK
            .with(|cache| {
                let s = cache.0.take();
                let sz = s.as_ref().map(|s| s.size()).unwrap_or(0);
                cache.0.set(s);
                sz
            });
        assert!(cached_size >= big, "TLS should hold the bigger stack after size bump");

        clear_tls_stack();
        drain_stack_pool();
    }
}
