// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

//! WebAssembly trap handling, which is built on top of the lower-level
//! signalhandling mechanisms.

use super::trapcode::TrapCode;
use crate::instance::{InstanceHandle, SignalHandler};
use crate::vmcontext::{VMContext, VMFunctionBody, VMTrampoline};
use backtrace::Backtrace;
use std::any::Any;
use std::cell::Cell;
use std::error::Error;
use std::io;
use std::mem;
use std::ptr;
use std::sync::Once;

extern "C" {
    fn RegisterSetjmp(
        jmp_buf: *mut *const u8,
        callback: extern "C" fn(*mut u8),
        payload: *mut u8,
    ) -> i32;
    fn Unwind(jmp_buf: *const u8) -> !;
}

cfg_if::cfg_if! {
    if #[cfg(unix)] {
        use std::mem::MaybeUninit;

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
            libc::pthread_getattr_np(this_thread, &mut thread_attrs);
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
                    // We assume the guard page is 1 page, and pages are 4KiB.
                    if stackaddr - 4096 <= addr && addr < stackaddr + stacksize {
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
                    Unwind(jmp_buf)
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
                } else if #[cfg(all(target_os = "linux", target_arch = "aarch64"))] {
                    let cx = &*(cx as *const libc::ucontext_t);
                    cx.uc_mcontext.pc as *const u8
                } else if #[cfg(target_os = "macos")] {
                    let cx = &*(cx as *const libc::ucontext_t);
                    (*cx.uc_mcontext).__ss.__rip as *const u8
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
                let jmp_buf = info.handle_trap(
                    (*(*exception_info).ContextRecord).Rip as *const u8,
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
                    Unwind(jmp_buf)
                }
            })
        }
    }
}

/// This function performs the low-overhead signal handler initialization that
/// we want to do eagerly to ensure a more-deterministic global process state.
///
/// This is especially relevant for signal handlers since handler ordering
/// depends on installation order: the wasm signal handler must run *before*
/// the other crash handlers and since POSIX signal handlers work LIFO, this
/// function needs to be called at the end of the startup process, after other
/// handlers have been installed. This function can thus be called multiple
/// times, having no effect after the first call.
pub fn init_traps() {
    static INIT: Once = Once::new();
    INIT.call_once(real_init);
}

fn real_init() {
    unsafe {
        platform_init();
    }
}

/// Raises a user-defined trap immediately.
///
/// This function performs as-if a wasm trap was just executed, only the trap
/// has a dynamic payload associated with it which is user-provided. This trap
/// payload is then returned from `wasmer_call` and `wasmer_call_trampoline`
/// below.
///
/// # Safety
///
/// Only safe to call when wasm code is on the stack, aka `wasmer_call` or
/// `wasmer_call_trampoline` must have been previously called.
pub unsafe fn raise_user_trap(data: Box<dyn Error + Send + Sync>) -> ! {
    tls::with(|info| info.unwrap().unwind_with(UnwindReason::UserTrap(data)))
}

/// Raises a trap from inside library code immediately.
///
/// This function performs as-if a wasm trap was just executed. This trap
/// payload is then returned from `wasmer_call` and `wasmer_call_trampoline`
/// below.
///
/// # Safety
///
/// Only safe to call when wasm code is on the stack, aka `wasmer_call` or
/// `wasmer_call_trampoline` must have been previously called.
pub unsafe fn raise_lib_trap(trap: Trap) -> ! {
    tls::with(|info| info.unwrap().unwind_with(UnwindReason::LibTrap(trap)))
}

/// Carries a Rust panic across wasm code and resumes the panic on the other
/// side.
///
/// # Safety
///
/// Only safe to call when wasm code is on the stack, aka `wasmer_call` or
/// `wasmer_call_trampoline` must have been previously called.
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

    /// A trap raised from machine code generated from Wasm
    Wasm {
        /// The program counter in generated code where this trap happened.
        pc: usize,
        /// Native stack backtrace at the time the trap occurred
        backtrace: Backtrace,
        /// Optional trapcode associated to the signal that caused the trap
        signal_trap: Option<TrapCode>,
    },

    /// A trap raised manually from the Wasmer VM
    Runtime {
        /// Code of the trap.
        trap_code: TrapCode,
        /// Native stack backtrace at the time the trap occurred
        backtrace: Backtrace,
    },
}

impl Trap {
    /// Construct a new VM `Trap` with the given the program counter, backtrace and an optional
    /// trap code associated with the signal received from the kernel.
    /// Wasm traps are Traps that are triggered by the chip when running generated
    /// code for a Wasm function.
    pub fn new_from_wasm(pc: usize, backtrace: Backtrace, signal_trap: Option<TrapCode>) -> Self {
        Self::Wasm {
            pc,
            backtrace,
            signal_trap,
        }
    }

    /// Construct a new runtime `Trap` with the given trap code.
    /// Runtime traps are Traps that are triggered manually from the VM.
    ///
    /// Internally saves a backtrace when constructed.
    pub fn new_from_runtime(trap_code: TrapCode) -> Self {
        let backtrace = Backtrace::new_unresolved();
        Self::Runtime {
            trap_code,
            backtrace,
        }
    }

    /// Construct a new Out of Memory (OOM) `Trap`.
    ///
    /// Internally saves a backtrace when constructed.
    pub fn new_from_user(error: Box<dyn Error + Send + Sync>) -> Self {
        Self::User(error)
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
    vmctx: *mut VMContext,
    trampoline: VMTrampoline,
    callee: *const VMFunctionBody,
    values_vec: *mut u8,
) -> Result<(), Trap> {
    catch_traps(vmctx, || {
        mem::transmute::<_, extern "C" fn(*mut VMContext, *const VMFunctionBody, *mut u8)>(
            trampoline,
        )(vmctx, callee, values_vec)
    })
}

/// Catches any wasm traps that happen within the execution of `closure`,
/// returning them as a `Result`.
///
/// # Safety
///
/// Highly unsafe since `closure` won't have any destructors run.
pub unsafe fn catch_traps<F>(vmctx: *mut VMContext, mut closure: F) -> Result<(), Trap>
where
    F: FnMut(),
{
    // Ensure that we have our sigaltstack installed.
    #[cfg(unix)]
    setup_unix_sigaltstack()?;

    return CallThreadState::new(vmctx).with(|cx| {
        RegisterSetjmp(
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
    vmctx: *mut VMContext,
    mut closure: F,
) -> Result<R, Trap>
where
    F: FnMut() -> R,
{
    let mut global_results = mem::MaybeUninit::<R>::uninit();
    catch_traps(vmctx, || {
        global_results.as_mut_ptr().write(closure());
    })?;
    Ok(global_results.assume_init())
}

/// Temporary state stored on the stack which is registered in the `tls` module
/// below for calls into wasm.
pub struct CallThreadState {
    unwind: Cell<UnwindReason>,
    jmp_buf: Cell<*const u8>,
    reset_guard_page: Cell<bool>,
    prev: Option<*const CallThreadState>,
    vmctx: *mut VMContext,
    handling_trap: Cell<bool>,
}

enum UnwindReason {
    None,
    Panic(Box<dyn Any + Send>),
    UserTrap(Box<dyn Error + Send + Sync>),
    LibTrap(Trap),
    RuntimeTrap {
        backtrace: Backtrace,
        pc: usize,
        signal_trap: Option<TrapCode>,
    },
}

impl CallThreadState {
    fn new(vmctx: *mut VMContext) -> Self {
        Self {
            unwind: Cell::new(UnwindReason::None),
            vmctx,
            jmp_buf: Cell::new(ptr::null()),
            reset_guard_page: Cell::new(false),
            prev: None,
            handling_trap: Cell::new(false),
        }
    }

    fn with(mut self, closure: impl FnOnce(&Self) -> i32) -> Result<(), Trap> {
        tls::with(|prev| {
            self.prev = prev.map(|p| p as *const _);
            let ret = tls::set(&self, || closure(&self));
            match self.unwind.replace(UnwindReason::None) {
                UnwindReason::None => {
                    debug_assert_eq!(ret, 1);
                    Ok(())
                }
                UnwindReason::UserTrap(data) => {
                    debug_assert_eq!(ret, 0);
                    Err(Trap::new_from_user(data))
                }
                UnwindReason::LibTrap(trap) => Err(trap),
                UnwindReason::RuntimeTrap {
                    backtrace,
                    pc,
                    signal_trap,
                } => {
                    debug_assert_eq!(ret, 0);
                    Err(Trap::new_from_wasm(pc, backtrace, signal_trap))
                }
                UnwindReason::Panic(panic) => {
                    debug_assert_eq!(ret, 0);
                    std::panic::resume_unwind(panic)
                }
            }
        })
    }

    fn any_instance(&self, func: impl Fn(&InstanceHandle) -> bool) -> bool {
        unsafe {
            if func(&InstanceHandle::from_vmctx(self.vmctx)) {
                return true;
            }
            match self.prev {
                Some(prev) => (*prev).any_instance(func),
                None => false,
            }
        }
    }

    fn unwind_with(&self, reason: UnwindReason) -> ! {
        self.unwind.replace(reason);
        unsafe {
            Unwind(self.jmp_buf.get());
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
        call_handler: impl Fn(&SignalHandler) -> bool,
    ) -> *const u8 {
        // If we hit a fault while handling a previous trap, that's quite bad,
        // so bail out and let the system handle this recursive segfault.
        //
        // Otherwise flag ourselves as handling a trap, do the trap handling,
        // and reset our trap handling flag.
        if self.handling_trap.replace(true) {
            return ptr::null();
        }

        // First up see if any instance registered has a custom trap handler,
        // in which case run them all. If anything handles the trap then we
        // return that the trap was handled.
        let any_instance = self.any_instance(|i| {
            let handler = match i.instance().signal_handler.replace(None) {
                Some(handler) => handler,
                None => return false,
            };
            let result = call_handler(&handler);
            i.instance().signal_handler.set(Some(handler));
            result
        });

        if any_instance {
            self.handling_trap.set(false);
            return 1 as *const _;
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
        self.unwind.replace(UnwindReason::RuntimeTrap {
            backtrace,
            signal_trap,
            pc: pc as usize,
        });
        self.handling_trap.set(false);
        self.jmp_buf.get()
    }
}

impl Drop for CallThreadState {
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
    use std::cell::Cell;
    use std::ptr;

    thread_local!(static PTR: Cell<*const CallThreadState> = Cell::new(ptr::null()));

    /// Configures thread local state such that for the duration of the
    /// execution of `closure` any call to `with` will yield `ptr`, unless this
    /// is recursively called again.
    pub fn set<R>(ptr: &CallThreadState, closure: impl FnOnce() -> R) -> R {
        struct Reset<'a, T: Copy>(&'a Cell<T>, T);

        impl<T: Copy> Drop for Reset<'_, T> {
            fn drop(&mut self) {
                self.0.set(self.1);
            }
        }

        PTR.with(|p| {
            let _r = Reset(p, p.replace(ptr));
            closure()
        })
    }

    /// Returns the last pointer configured with `set` above. Panics if `set`
    /// has not been previously called.
    pub fn with<R>(closure: impl FnOnce(Option<&CallThreadState>) -> R) -> R {
        PTR.with(|ptr| {
            let p = ptr.get();
            unsafe { closure(if p.is_null() { None } else { Some(&*p) }) }
        })
    }
}

/// A module for registering a custom alternate signal stack (sigaltstack).
///
/// Rust's libstd installs an alternate stack with size `SIGSTKSZ`, which is not
/// always large enough for our signal handling code. Override it by creating
/// and registering our own alternate stack that is large enough and has a guard
/// page.
#[cfg(unix)]
fn setup_unix_sigaltstack() -> Result<(), Trap> {
    use std::cell::RefCell;
    use std::convert::TryInto;
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
        let page_size: usize = libc::sysconf(libc::_SC_PAGESIZE).try_into().unwrap();
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
            return Err(Trap::new_from_runtime(TrapCode::VMOutOfMemory));
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
