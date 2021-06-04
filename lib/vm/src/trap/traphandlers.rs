// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

//! WebAssembly trap handling, which is built on top of the lower-level
//! signalhandling mechanisms.

use super::trapcode::TrapCode;
use crate::vmcontext::{VMFunctionBody, VMFunctionEnvironment, VMTrampoline};
use backtrace::Backtrace;
use std::any::Any;
use std::cell::{Cell, UnsafeCell};
use std::error::Error;
use std::io;
use std::mem::{self, MaybeUninit};
use std::ptr;
use std::sync::Once;
pub use tls::TlsRestore;

cfg_if::cfg_if! {
    if #[cfg(unix)] {
        /// Function which may handle custom signals while processing traps.
        pub type TrapHandlerFn = dyn Fn(libc::c_int, *const libc::siginfo_t, *const libc::c_void) -> bool;
    } else if #[cfg(target_os = "windows")] {
        /// Function which may handle custom signals while processing traps.
        pub type TrapHandlerFn = dyn Fn(winapi::um::winnt::PEXCEPTION_POINTERS) -> bool;
    }
}

extern "C" {
    fn wasmer_register_setjmp(
        jmp_buf: *mut *const u8,
        callback: extern "C" fn(*mut u8),
        payload: *mut u8,
    ) -> i32;
    fn wasmer_unwind(jmp_buf: *const u8) -> !;
}

cfg_if::cfg_if! {
    if #[cfg(unix)] {
        static mut PREV_SIGSEGV: MaybeUninit<libc::sigaction> = MaybeUninit::uninit();
        static mut PREV_SIGBUS: MaybeUninit<libc::sigaction> = MaybeUninit::uninit();
        static mut PREV_SIGILL: MaybeUninit<libc::sigaction> = MaybeUninit::uninit();
        static mut PREV_SIGFPE: MaybeUninit<libc::sigaction> = MaybeUninit::uninit();

        unsafe fn platform_init() {
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
            if cfg!(target_arch = "arm") || cfg!(target_os = "macos") {
                register(&mut PREV_SIGBUS, libc::SIGBUS);
            }
        }

        #[cfg(target_os = "macos")]
        unsafe fn thread_stack() -> (usize, usize) {
            let this_thread = libc::pthread_self();
            let stackaddr = libc::pthread_get_stackaddr_np(this_thread);
            let stacksize = libc::pthread_get_stacksize_np(this_thread);
            (stackaddr as usize - stacksize, stacksize)
        }

        #[cfg(not(target_os = "macos"))]
        unsafe fn thread_stack() -> (usize, usize) {
            let this_thread = libc::pthread_self();
            let mut thread_attrs: libc::pthread_attr_t = mem::zeroed();
            #[cfg(not(target_os = "freebsd"))]
            libc::pthread_getattr_np(this_thread, &mut thread_attrs);
            #[cfg(target_os = "freebsd")]
            libc::pthread_attr_get_np(this_thread, &mut thread_attrs);
            let mut stackaddr: *mut libc::c_void = ptr::null_mut();
            let mut stacksize: libc::size_t = 0;
            libc::pthread_attr_getstack(&thread_attrs, &mut stackaddr, &mut stacksize);
            (stackaddr as usize, stacksize)
        }

        unsafe extern "C" fn trap_handler(
            signum: libc::c_int,
            siginfo: *mut libc::siginfo_t,
            context: *mut libc::c_void,
        ) {
            let previous = match signum {
                libc::SIGSEGV => &PREV_SIGSEGV,
                libc::SIGBUS => &PREV_SIGBUS,
                libc::SIGFPE => &PREV_SIGFPE,
                libc::SIGILL => &PREV_SIGILL,
                _ => panic!("unknown signal: {}", signum),
            };
            // We try to get the Code trap associated to this signal
            let maybe_signal_trap = match signum {
                libc::SIGSEGV | libc::SIGBUS => {
                    let addr = (*siginfo).si_addr() as usize;
                    let (stackaddr, stacksize) = thread_stack();
                    // The stack and its guard page covers the
                    // range [stackaddr - guard pages .. stackaddr + stacksize).
                    // We assume the guard page is 1 page, and pages are 4KiB (or 16KiB in Apple Silicon)
                    if stackaddr - region::page::size() <= addr && addr < stackaddr + stacksize {
                        Some(TrapCode::StackOverflow)
                    } else {
                        Some(TrapCode::HeapAccessOutOfBounds)
                    }
                }
                _ => None,
            };
            let handled = tls::with(|info| {
                // If no wasm code is executing, we don't handle this as a wasm
                // trap.
                let info = match info {
                    Some(info) => info,
                    None => return false,
                };

                // If we hit an exception while handling a previous trap, that's
                // quite bad, so bail out and let the system handle this
                // recursive segfault.
                //
                // Otherwise flag ourselves as handling a trap, do the trap
                // handling, and reset our trap handling flag. Then we figure
                // out what to do based on the result of the trap handling.
                let jmp_buf = info.handle_trap(
                    get_pc(context),
                    false,
                    maybe_signal_trap,
                    |handler| handler(signum, siginfo, context),
                );

                // Figure out what to do based on the result of this handling of
                // the trap. Note that our sentinel value of 1 means that the
                // exception was handled by a custom exception handler, so we
                // keep executing.
                if jmp_buf.is_null() {
                    false
                } else if jmp_buf as usize == 1 {
                    true
                } else {
                    wasmer_unwind(jmp_buf)
                }
            });

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
            } else if previous.sa_sigaction == libc::SIG_DFL ||
                previous.sa_sigaction == libc::SIG_IGN
            {
                libc::sigaction(signum, previous, ptr::null_mut());
            } else {
                mem::transmute::<usize, extern "C" fn(libc::c_int)>(
                    previous.sa_sigaction
                )(signum)
            }
        }

        unsafe fn get_pc(cx: *mut libc::c_void) -> *const u8 {
            cfg_if::cfg_if! {
                if #[cfg(all(target_os = "linux", target_arch = "x86_64"))] {
                    let cx = &*(cx as *const libc::ucontext_t);
                    cx.uc_mcontext.gregs[libc::REG_RIP as usize] as *const u8
                } else if #[cfg(all(target_os = "linux", target_arch = "x86"))] {
                    let cx = &*(cx as *const libc::ucontext_t);
                    cx.uc_mcontext.gregs[libc::REG_EIP as usize] as *const u8
                } else if #[cfg(all(target_os = "android", target_arch = "x86"))] {
                    let cx = &*(cx as *const libc::ucontext_t);
                    cx.uc_mcontext.gregs[libc::REG_EIP as usize] as *const u8
                } else if #[cfg(all(target_os = "linux", target_arch = "aarch64"))] {
                    let cx = &*(cx as *const libc::ucontext_t);
                    cx.uc_mcontext.pc as *const u8
                } else if #[cfg(all(target_os = "android", target_arch = "aarch64"))] {
                    let cx = &*(cx as *const libc::ucontext_t);
                    cx.uc_mcontext.pc as *const u8
                } else if #[cfg(all(target_os = "macos", target_arch = "x86_64"))] {
                    let cx = &*(cx as *const libc::ucontext_t);
                    (*cx.uc_mcontext).__ss.__rip as *const u8
                } else if #[cfg(all(target_os = "macos", target_arch = "aarch64"))] {
                    use std::mem;
                    // TODO: This should be integrated into rust/libc
                    // Related issue: https://github.com/rust-lang/libc/issues/1977
                    #[allow(non_camel_case_types)]
                    pub struct __darwin_arm_thread_state64 {
                        pub __x: [u64; 29], /* General purpose registers x0-x28 */
                        pub __fp: u64,    /* Frame pointer x29 */
                        pub __lr: u64,    /* Link register x30 */
                        pub __sp: u64,    /* Stack pointer x31 */
                        pub __pc: u64,   /* Program counter */
                        pub __cpsr: u32,  /* Current program status register */
                        pub __pad: u32,   /* Same size for 32-bit or 64-bit clients */
                    }

                    let cx = &*(cx as *const libc::ucontext_t);
                    let uc_mcontext = mem::transmute::<_, *const __darwin_arm_thread_state64>(&(*cx.uc_mcontext).__ss);
                    (*uc_mcontext).__pc as *const u8
                } else if #[cfg(all(target_os = "freebsd", target_arch = "x86_64"))] {
                    let cx = &*(cx as *const libc::ucontext_t);
                    cx.uc_mcontext.mc_rip as *const u8
                } else if #[cfg(all(target_os = "freebsd", target_arch = "aarch64"))] {
                    #[repr(align(16))]
                    #[allow(non_camel_case_types)]
                    pub struct gpregs {
                        pub gp_x: [libc::register_t; 30],
                        pub gp_lr: libc::register_t,
                        pub gp_sp: libc::register_t,
                        pub gp_elr: libc::register_t,
                        pub gp_spsr: u32,
                        pub gp_pad: libc::c_int,
                    };
                    #[repr(align(16))]
                    #[allow(non_camel_case_types)]
                    pub struct fpregs {
                        pub fp_q: [u128; 32],
                        pub fp_sr: u32,
                        pub fp_cr: u32,
                        pub fp_flags: libc::c_int,
                        pub fp_pad: libc::c_int,
                    };
                    #[repr(align(16))]
                    #[allow(non_camel_case_types)]
                    pub struct mcontext_t {
                        pub mc_gpregs: gpregs,
                        pub mc_fpregs: fpregs,
                        pub mc_flags: libc::c_int,
                        pub mc_pad: libc::c_int,
                        pub mc_spare: [u64; 8],
                    };
                    #[repr(align(16))]
                    #[allow(non_camel_case_types)]
                    pub struct ucontext_t {
                        pub uc_sigmask: libc::sigset_t,
                        pub uc_mcontext: mcontext_t,
                        pub uc_link: *mut ucontext_t,
                        pub uc_stack: libc::stack_t,
                        pub uc_flags: libc::c_int,
                        __spare__: [libc::c_int; 4],
                    }

                    let cx = &*(cx as *const ucontext_t);
                    cx.uc_mcontext.mc_gpregs.gp_elr as *const u8
                } else {
                    compile_error!("unsupported platform");
                }
            }
        }
    } else if #[cfg(target_os = "windows")] {
        use winapi::um::errhandlingapi::*;
        use winapi::um::winnt::*;
        use winapi::um::minwinbase::*;
        use winapi::vc::excpt::*;

        unsafe fn platform_init() {
            // our trap handler needs to go first, so that we can recover from
            // wasm faults and continue execution, so pass `1` as a true value
            // here.
            if AddVectoredExceptionHandler(1, Some(exception_handler)).is_null() {
                panic!("failed to add exception handler: {}", io::Error::last_os_error());
            }
        }

        unsafe extern "system" fn exception_handler(
            exception_info: PEXCEPTION_POINTERS
        ) -> LONG {
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

            // This is basically the same as the unix version above, only with a
            // few parameters tweaked here and there.
            tls::with(|info| {
                let info = match info {
                    Some(info) => info,
                    None => return EXCEPTION_CONTINUE_SEARCH,
                };
                #[cfg(target_pointer_width = "32")]
                let pc = (*(*exception_info).ContextRecord).Eip as *const u8;

                #[cfg(target_pointer_width = "64")]
                let pc = (*(*exception_info).ContextRecord).Rip as *const u8;

                let jmp_buf = info.handle_trap(
                    pc,
                    record.ExceptionCode == EXCEPTION_STACK_OVERFLOW,
                    // TODO: fix the signal trap associated to memory access in Windows
                    None,
                    |handler| handler(exception_info),
                );
                if jmp_buf.is_null() {
                    EXCEPTION_CONTINUE_SEARCH
                } else if jmp_buf as usize == 1 {
                    EXCEPTION_CONTINUE_EXECUTION
                } else {
                    wasmer_unwind(jmp_buf)
                }
            })
        }
    }
}

/// Globally-set callback to determine whether a program counter is actually a
/// wasm trap.
///
/// This is initialized during `init_traps` below. The definition lives within
/// `wasmer` currently.
static mut IS_WASM_PC: fn(usize) -> bool = |_| false;

/// This function is required to be called before any WebAssembly is entered.
/// This will configure global state such as signal handlers to prepare the
/// process to receive wasm traps.
///
/// This function must not only be called globally once before entering
/// WebAssembly but it must also be called once-per-thread that enters
/// WebAssembly. Currently in wasmer's integration this function is called on
/// creation of a `Store`.
///
/// The `is_wasm_pc` argument is used when a trap happens to determine if a
/// program counter is the pc of an actual wasm trap or not. This is then used
/// to disambiguate faults that happen due to wasm and faults that happen due to
/// bugs in Rust or elsewhere.
pub fn init_traps(is_wasm_pc: fn(usize) -> bool) {
    static INIT: Once = Once::new();
    INIT.call_once(|| unsafe {
        IS_WASM_PC = is_wasm_pc;
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
    tls::with(|info| info.unwrap().unwind_with(UnwindReason::UserTrap(data)))
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
    tls::with(|info| info.unwrap().unwind_with(UnwindReason::LibTrap(trap)))
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
    tls::with(|info| info.unwrap().unwind_with(UnwindReason::Panic(payload)))
}

#[cfg(target_os = "windows")]
fn reset_guard_page() {
    extern "C" {
        fn _resetstkoflw() -> winapi::ctypes::c_int;
    }

    // We need to restore guard page under stack to handle future stack overflows properly.
    // https://docs.microsoft.com/en-us/cpp/c-runtime-library/reference/resetstkoflw?view=vs-2019
    if unsafe { _resetstkoflw() } == 0 {
        panic!("failed to restore stack guard page");
    }
}

#[cfg(not(target_os = "windows"))]
fn reset_guard_page() {}

/// Stores trace message with backtrace.
#[derive(Debug)]
pub enum Trap {
    /// A user-raised trap through `raise_user_trap`.
    User(Box<dyn Error + Send + Sync>),

    /// A trap raised from the Wasm generated code
    ///
    /// Note: this trap is deterministic (assuming a deterministic host implementation)
    Wasm {
        /// The program counter in generated code where this trap happened.
        pc: usize,
        /// Native stack backtrace at the time the trap occurred
        backtrace: Backtrace,
        /// Optional trapcode associated to the signal that caused the trap
        signal_trap: Option<TrapCode>,
    },

    /// A trap raised from a wasm libcall
    ///
    /// Note: this trap is deterministic (assuming a deterministic host implementation)
    Lib {
        /// Code of the trap.
        trap_code: TrapCode,
        /// Native stack backtrace at the time the trap occurred
        backtrace: Backtrace,
    },

    /// A trap indicating that the runtime was unable to allocate sufficient memory.
    ///
    /// Note: this trap is nondeterministic, since it depends on the host system.
    OOM {
        /// Native stack backtrace at the time the OOM occurred
        backtrace: Backtrace,
    },
}

impl Trap {
    /// Construct a new Wasm trap with the given source location and backtrace.
    ///
    /// Internally saves a backtrace when constructed.
    pub fn wasm(pc: usize, backtrace: Backtrace, signal_trap: Option<TrapCode>) -> Self {
        Trap::Wasm {
            pc,
            backtrace,
            signal_trap,
        }
    }

    /// Construct a new Wasm trap with the given trap code.
    ///
    /// Internally saves a backtrace when constructed.
    pub fn lib(trap_code: TrapCode) -> Self {
        let backtrace = Backtrace::new_unresolved();
        Trap::Lib {
            trap_code,
            backtrace,
        }
    }

    /// Construct a new OOM trap with the given source location and trap code.
    ///
    /// Internally saves a backtrace when constructed.
    pub fn oom() -> Self {
        let backtrace = Backtrace::new_unresolved();
        Trap::OOM { backtrace }
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
pub unsafe fn wasmer_call_trampoline(
    trap_handler: &impl TrapHandler,
    vmctx: VMFunctionEnvironment,
    trampoline: VMTrampoline,
    callee: *const VMFunctionBody,
    values_vec: *mut u8,
) -> Result<(), Trap> {
    catch_traps(trap_handler, || {
        mem::transmute::<_, extern "C" fn(VMFunctionEnvironment, *const VMFunctionBody, *mut u8)>(
            trampoline,
        )(vmctx, callee, values_vec);
    })
}

/// Catches any wasm traps that happen within the execution of `closure`,
/// returning them as a `Result`.
///
/// Highly unsafe since `closure` won't have any dtors run.
pub unsafe fn catch_traps<F>(trap_handler: &dyn TrapHandler, mut closure: F) -> Result<(), Trap>
where
    F: FnMut(),
{
    return CallThreadState::new(trap_handler).with(|cx| {
        wasmer_register_setjmp(
            cx.jmp_buf.as_ptr(),
            call_closure::<F>,
            &mut closure as *mut F as *mut u8,
        )
    });

    extern "C" fn call_closure<F>(payload: *mut u8)
    where
        F: FnMut(),
    {
        unsafe { (*(payload as *mut F))() }
    }
}

/// Catches any wasm traps that happen within the execution of `closure`,
/// returning them as a `Result`, with the closure contents.
///
/// The main difference from this method and `catch_traps`, is that is able
/// to return the results from the closure.
///
/// # Safety
///
/// Check [`catch_traps`].
pub unsafe fn catch_traps_with_result<F, R>(
    trap_handler: &dyn TrapHandler,
    mut closure: F,
) -> Result<R, Trap>
where
    F: FnMut() -> R,
{
    let mut global_results = MaybeUninit::<R>::uninit();
    catch_traps(trap_handler, || {
        global_results.as_mut_ptr().write(closure());
    })?;
    Ok(global_results.assume_init())
}

/// Temporary state stored on the stack which is registered in the `tls` module
/// below for calls into wasm.
pub struct CallThreadState<'a> {
    unwind: UnsafeCell<MaybeUninit<UnwindReason>>,
    jmp_buf: Cell<*const u8>,
    reset_guard_page: Cell<bool>,
    prev: Cell<tls::Ptr>,
    trap_handler: &'a (dyn TrapHandler + 'a),
    handling_trap: Cell<bool>,
}

/// A package of functionality needed by `catch_traps` to figure out what to do
/// when handling a trap.
///
/// Note that this is an `unsafe` trait at least because it's being run in the
/// context of a synchronous signal handler, so it needs to be careful to not
/// access too much state in answering these queries.
pub unsafe trait TrapHandler {
    /// Converts this object into an `Any` to dynamically check its type.
    fn as_any(&self) -> &dyn Any;

    /// Uses `call` to call a custom signal handler, if one is specified.
    ///
    /// Returns `true` if `call` returns true, otherwise returns `false`.
    fn custom_trap_handler(&self, call: &dyn Fn(&TrapHandlerFn) -> bool) -> bool;
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

impl<'a> CallThreadState<'a> {
    #[inline]
    fn new(trap_handler: &'a (dyn TrapHandler + 'a)) -> CallThreadState<'a> {
        Self {
            unwind: UnsafeCell::new(MaybeUninit::uninit()),
            jmp_buf: Cell::new(ptr::null()),
            reset_guard_page: Cell::new(false),
            prev: Cell::new(ptr::null()),
            trap_handler,
            handling_trap: Cell::new(false),
        }
    }

    fn with(self, closure: impl FnOnce(&CallThreadState) -> i32) -> Result<(), Trap> {
        let ret = tls::set(&self, || closure(&self))?;
        if ret != 0 {
            return Ok(());
        }
        // We will only reach this path if ret == 0. And that will
        // only happen if a trap did happen. As such, it's safe to
        // assume that the `unwind` field is already initialized
        // at this moment.
        match unsafe { (*self.unwind.get()).as_ptr().read() } {
            UnwindReason::UserTrap(data) => Err(Trap::User(data)),
            UnwindReason::LibTrap(trap) => Err(trap),
            UnwindReason::WasmTrap {
                backtrace,
                pc,
                signal_trap,
            } => Err(Trap::wasm(pc, backtrace, signal_trap)),
            UnwindReason::Panic(panic) => std::panic::resume_unwind(panic),
        }
    }

    fn unwind_with(&self, reason: UnwindReason) -> ! {
        unsafe {
            (*self.unwind.get()).as_mut_ptr().write(reason);
            wasmer_unwind(self.jmp_buf.get());
        }
    }

    /// Trap handler using our thread-local state.
    ///
    /// * `pc` - the program counter the trap happened at
    /// * `reset_guard_page` - whether or not to reset the guard page,
    ///   currently Windows specific
    /// * `call_handler` - a closure used to invoke the platform-specific
    ///   signal handler for each instance, if available.
    ///
    /// Attempts to handle the trap if it's a wasm trap. Returns a few
    /// different things:
    ///
    /// * null - the trap didn't look like a wasm trap and should continue as a
    ///   trap
    /// * 1 as a pointer - the trap was handled by a custom trap handler on an
    ///   instance, and the trap handler should quickly return.
    /// * a different pointer - a jmp_buf buffer to longjmp to, meaning that
    ///   the wasm trap was succesfully handled.
    fn handle_trap(
        &self,
        pc: *const u8,
        reset_guard_page: bool,
        signal_trap: Option<TrapCode>,
        call_handler: impl Fn(&TrapHandlerFn) -> bool,
    ) -> *const u8 {
        // If we hit a fault while handling a previous trap, that's quite bad,
        // so bail out and let the system handle this recursive segfault.
        //
        // Otherwise flag ourselves as handling a trap, do the trap handling,
        // and reset our trap handling flag.
        if self.handling_trap.replace(true) {
            return ptr::null();
        }

        // First up see if we have a custom trap handler,
        // in which case run it. If anything handles the trap then we
        // return that the trap was handled.
        if self.trap_handler.custom_trap_handler(&call_handler) {
            return 1 as *const _;
        }

        // If this fault wasn't in wasm code, then it's not our problem
        if unsafe { !IS_WASM_PC(pc as _) } {
            return ptr::null();
        }

        // TODO: stack overflow can happen at any random time (i.e. in malloc()
        // in memory.grow) and it's really hard to determine if the cause was
        // stack overflow and if it happened in WebAssembly module.
        //
        // So, let's assume that any untrusted code called from WebAssembly
        // doesn't trap. Then, if we have called some WebAssembly code, it
        // means the trap is stack overflow.
        if self.jmp_buf.get().is_null() {
            self.handling_trap.set(false);
            return ptr::null();
        }
        let backtrace = Backtrace::new_unresolved();
        self.reset_guard_page.set(reset_guard_page);
        unsafe {
            (*self.unwind.get())
                .as_mut_ptr()
                .write(UnwindReason::WasmTrap {
                    backtrace,
                    signal_trap,
                    pc: pc as usize,
                });
        }
        self.handling_trap.set(false);
        self.jmp_buf.get()
    }
}

impl<'a> Drop for CallThreadState<'a> {
    fn drop(&mut self) {
        if self.reset_guard_page.get() {
            reset_guard_page();
        }
    }
}

// A private inner module for managing the TLS state that we require across
// calls in wasm. The WebAssembly code is called from C++ and then a trap may
// happen which requires us to read some contextual state to figure out what to
// do with the trap. This `tls` module is used to persist that information from
// the caller to the trap site.
mod tls {
    use super::CallThreadState;
    use crate::Trap;
    use std::mem;
    use std::ptr;

    pub use raw::Ptr;

    // An even *more* inner module for dealing with TLS. This actually has the
    // thread local variable and has functions to access the variable.
    //
    // Note that this is specially done to fully encapsulate that the accessors
    // for tls must not be inlined. Wasmer's async support will employ stack
    // switching which can resume execution on different OS threads. This means
    // that borrows of our TLS pointer must never live across accesses because
    // otherwise the access may be split across two threads and cause unsafety.
    //
    // This also means that extra care is taken by the runtime to save/restore
    // these TLS values when the runtime may have crossed threads.
    mod raw {
        use super::CallThreadState;
        use crate::Trap;
        use std::cell::Cell;
        use std::ptr;

        pub type Ptr = *const CallThreadState<'static>;

        // The first entry here is the `Ptr` which is what's used as part of the
        // public interface of this module. The second entry is a boolean which
        // allows the runtime to perform per-thread initialization if necessary
        // for handling traps (e.g. setting up ports on macOS and sigaltstack on
        // Unix).
        thread_local!(static PTR: Cell<(Ptr, bool)> = Cell::new((ptr::null(), false)));

        #[inline(never)] // see module docs for why this is here
        pub fn replace(val: Ptr) -> Result<Ptr, Trap> {
            PTR.with(|p| {
                // When a new value is configured that means that we may be
                // entering WebAssembly so check to see if this thread has
                // performed per-thread initialization for traps.
                let (prev, mut initialized) = p.get();
                if !initialized {
                    super::super::lazy_per_thread_init()?;
                    initialized = true;
                }
                p.set((val, initialized));
                Ok(prev)
            })
        }

        #[inline(never)] // see module docs for why this is here
        pub fn get() -> Ptr {
            PTR.with(|p| p.get().0)
        }
    }

    /// Opaque state used to help control TLS state across stack switches for
    /// async support.
    pub struct TlsRestore(raw::Ptr);

    impl TlsRestore {
        /// Takes the TLS state that is currently configured and returns a
        /// token that is used to replace it later.
        ///
        /// # Safety
        ///
        /// This is not a safe operation since it's intended to only be used
        /// with stack switching found with fibers and async wasmer.
        pub unsafe fn take() -> Result<TlsRestore, Trap> {
            // Our tls pointer must be set at this time, and it must not be
            // null. We need to restore the previous pointer since we're
            // removing ourselves from the call-stack, and in the process we
            // null out our own previous field for safety in case it's
            // accidentally used later.
            let raw = raw::get();
            assert!(!raw.is_null());
            let prev = (*raw).prev.replace(ptr::null());
            raw::replace(prev)?;
            Ok(TlsRestore(raw))
        }

        /// Restores a previous tls state back into this thread's TLS.
        ///
        /// # Safety
        ///
        /// This is unsafe because it's intended to only be used within the
        /// context of stack switching within wasmer.
        pub unsafe fn replace(self) -> Result<(), super::Trap> {
            // We need to configure our previous TLS pointer to whatever is in
            // TLS at this time, and then we set the current state to ourselves.
            let prev = raw::get();
            assert!((*self.0).prev.get().is_null());
            (*self.0).prev.set(prev);
            raw::replace(self.0)?;
            Ok(())
        }
    }

    /// Configures thread local state such that for the duration of the
    /// execution of `closure` any call to `with` will yield `ptr`, unless this
    /// is recursively called again.
    pub fn set<R>(state: &CallThreadState<'_>, closure: impl FnOnce() -> R) -> Result<R, Trap> {
        struct Reset<'a, 'b>(&'a CallThreadState<'b>);

        impl Drop for Reset<'_, '_> {
            #[inline]
            fn drop(&mut self) {
                raw::replace(self.0.prev.replace(ptr::null()))
                    .expect("tls should be previously initialized");
            }
        }

        // Note that this extension of the lifetime to `'static` should be
        // safe because we only ever access it below with an anonymous
        // lifetime, meaning `'static` never leaks out of this module.
        let ptr = unsafe { mem::transmute::<*const CallThreadState<'_>, _>(state) };
        let prev = raw::replace(ptr)?;
        state.prev.set(prev);
        let _reset = Reset(state);
        Ok(closure())
    }

    /// Returns the last pointer configured with `set` above. Panics if `set`
    /// has not been previously called and not returned.
    pub fn with<R>(closure: impl FnOnce(Option<&CallThreadState<'_>>) -> R) -> R {
        let p = raw::get();
        unsafe { closure(if p.is_null() { None } else { Some(&*p) }) }
    }
}

#[cfg(not(unix))]
pub fn lazy_per_thread_init() -> Result<(), Trap> {
    // Unused on Windows
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
    use std::cell::RefCell;
    use std::ptr::null_mut;

    thread_local! {
        /// Thread-local state is lazy-initialized on the first time it's used,
        /// and dropped when the thread exits.
        static TLS: RefCell<Tls> = RefCell::new(Tls::None);
    }

    /// The size of the sigaltstack (not including the guard, which will be
    /// added). Make this large enough to run our signal handlers.
    const MIN_STACK_SIZE: usize = 16 * 4096;

    enum Tls {
        None,
        Allocated {
            mmap_ptr: *mut libc::c_void,
            mmap_size: usize,
        },
        BigEnough,
    }

    return TLS.with(|slot| unsafe {
        let mut slot = slot.borrow_mut();
        match *slot {
            Tls::None => {}
            // already checked
            _ => return Ok(()),
        }
        // Check to see if the existing sigaltstack, if it exists, is big
        // enough. If so we don't need to allocate our own.
        let mut old_stack = mem::zeroed();
        let r = libc::sigaltstack(ptr::null(), &mut old_stack);
        assert_eq!(r, 0, "learning about sigaltstack failed");
        if old_stack.ss_flags & libc::SS_DISABLE == 0 && old_stack.ss_size >= MIN_STACK_SIZE {
            *slot = Tls::BigEnough;
            return Ok(());
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
            return Err(Trap::oom());
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

        *slot = Tls::Allocated {
            mmap_ptr: ptr,
            mmap_size: alloc_size,
        };
        Ok(())
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
