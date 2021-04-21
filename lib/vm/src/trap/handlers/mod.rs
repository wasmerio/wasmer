// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

//! WebAssembly trap handling, which is built on top of the lower-level
//! signalhandling mechanisms.

use super::code::TrapCode;
use crate::vmcontext::{VMFunctionBody, VMFunctionEnvironment, VMTrampoline};

use backtrace::Backtrace;
use std::any::Any;
use std::cell::{Cell, UnsafeCell};
use std::error::Error;
use std::mem::{self, MaybeUninit};
use std::ptr;
use std::sync::Once;

pub use self::tls::TlsRestore;

extern "C" {
    fn register_setjmp(
        jmp_buf: *mut *const u8,
        callback: extern "C" fn(*mut u8),
        payload: *mut u8,
    ) -> i32;
    fn unwind(jmp_buf: *const u8) -> !;
}

cfg_if::cfg_if! {
    // if #[cfg(target_os = "macos")] {
    //     mod macos;
    //     use macos as sys;
    // } else
    if #[cfg(unix)] {
        mod unix;
        use unix as sys;
    } else if #[cfg(target_os = "windows")] {
        mod windows;
        use windows as sys;
    }
}

pub use sys::SignalHandler;

/// Globally-set callback to determine whether a program counter is actually a
/// wasm trap.
///
/// This is initialized during `init_traps` below. The definition lives within
/// `wasmer` currently.
static mut IS_WASM_PC: fn(usize) -> bool = |_| true;

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
pub fn init_traps(is_wasm_pc: fn(usize) -> bool) -> Result<(), Trap> {
    // pub fn init_traps() -> Result<(), Trap> {
    static INIT: Once = Once::new();
    INIT.call_once(|| unsafe {
        IS_WASM_PC = is_wasm_pc;
        sys::platform_init();
    });
    sys::lazy_per_thread_init()
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
/// have been previously called. Additionally no Rust destructors can be on the
/// stack. They will be skipped and not executed.
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
/// have been previously called. Additionally no Rust destructors can be on the
/// stack. They will be skipped and not executed.
pub unsafe fn raise_lib_trap(trap: Trap) -> ! {
    tls::with(|info| info.unwrap().unwind_with(UnwindReason::LibTrap(trap)))
}

/// Carries a Rust panic across wasm code and resumes the panic on the other
/// side.
///
/// # Safety
///
/// Only safe to call when wasm code is on the stack, aka `catch_traps` must
/// have been previously called. Additionally no Rust destructors can be on the
/// stack. They will be skipped and not executed.
pub unsafe fn resume_panic(payload: Box<dyn Any + Send>) -> ! {
    tls::with(|info| info.unwrap().unwind_with(UnwindReason::Panic(payload)))
}

/// Stores trace message with backtrace.
#[derive(Debug)]
pub enum Trap {
    /// A user-raised trap through `raise_user_trap`.
    User(Box<dyn Error + Send + Sync>),

    /// A trap raised from the Wasm generated code
    ///
    /// Note: this trap is deterministic (assuming a deterministic host implementation)
    Wasm {
        /// The program counter in JIT code where this trap happened.
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
    /// Note: this trap is undeterministic, since it depends on the host system.
    OOM {
        /// Native stack backtrace at the time the OOM occurred
        backtrace: Backtrace,
    },
}

impl Trap {
    /// Construct a new Wasm trap with the given source location and backtrace.
    ///
    /// Internally saves a backtrace when constructed.
    pub fn wasm(pc: usize, backtrace: Backtrace) -> Self {
        Trap::Wasm {
            pc,
            backtrace,
            signal_trap: None,
        }
    }

    /// Construct a new Wasm trap with the given trap code.
    ///
    /// Internally saves a backtrace when constructed.
    pub fn lib(trap_code: TrapCode) -> Self {
        println!("TRAP::lib 0");
        let backtrace = Backtrace::new_unresolved();
        println!("TRAP::lib 1");
        Trap::Lib {
            trap_code,
            backtrace,
        }
    }

    /// Construct a new OOM trap with the given source location and trap code.
    ///
    /// Internally saves a backtrace when constructed.
    pub fn oom() -> Self {
        println!("TRAP::oom 0");
        let backtrace = Backtrace::new_unresolved();
        println!("TRAP::oom 1");
        Trap::OOM { backtrace }
    }
}

/// Catches any wasm traps that happen within the execution of `closure`,
/// returning them as a `Result`.
///
/// Highly unsafe since `closure` won't have any dtors run.
pub unsafe fn catch_traps<F>(trap_info: &dyn TrapInfo, mut closure: F) -> Result<(), Trap>
where
    F: FnMut(),
{
    return CallThreadState::new(trap_info).with(|cx| {
        register_setjmp(
            cx.jmp_buf.as_ptr(),
            call_closure::<F>,
            &mut closure as *mut F as *mut u8,
        )
    });

    extern "C" fn call_closure<F>(payload: *mut u8)
    where
        F: FnMut(),
    {
        println!("CALL CLOSURE");
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
    trap_info: &dyn TrapInfo,
    mut closure: F,
) -> Result<R, Trap>
where
    F: FnMut() -> R,
{
    let mut global_results = MaybeUninit::<R>::uninit();
    catch_traps(trap_info, || {
        global_results.as_mut_ptr().write(closure());
    })?;
    Ok(global_results.assume_init())
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
    trap_info: &impl TrapInfo,
    vmctx: VMFunctionEnvironment,
    trampoline: VMTrampoline,
    callee: *const VMFunctionBody,
    values_vec: *mut u8,
) -> Result<(), Trap> {
    catch_traps(trap_info, || {
        println!("CALLING TRAMPOLINE");
        mem::transmute::<_, extern "C" fn(VMFunctionEnvironment, *const VMFunctionBody, *mut u8)>(
            trampoline,
        )(vmctx, callee, values_vec);
        println!("TRAMPOLINE CALLED");
    })
}

/// Runs `func` with the last `trap_info` object registered by `catch_traps`.
///
/// Calls `func` with `None` if `catch_traps` wasn't previously called from this
/// stack frame.
pub fn with_last_info<R>(func: impl FnOnce(Option<&dyn Any>) -> R) -> R {
    tls::with(|state| func(state.map(|s| s.trap_info.as_any())))
}

/// Temporary state stored on the stack which is registered in the `tls` module
/// below for calls into wasm.
pub struct CallThreadState<'a> {
    unwind: UnsafeCell<MaybeUninit<UnwindReason>>,
    jmp_buf: Cell<*const u8>,
    handling_trap: Cell<bool>,
    trap_info: &'a (dyn TrapInfo + 'a),
    prev: Cell<tls::Ptr>,
}

/// A package of functionality needed by `catch_traps` to figure out what to do
/// when handling a trap.
///
/// Note that this is an `unsafe` trait at least because it's being run in the
/// context of a synchronous signal handler, so it needs to be careful to not
/// access too much state in answering these queries.
pub unsafe trait TrapInfo {
    /// Converts this object into an `Any` to dynamically check its type.
    fn as_any(&self) -> &dyn Any;

    /// Uses `call` to call a custom signal handler, if one is specified.
    ///
    /// Returns `true` if `call` returns true, otherwise returns `false`.
    fn custom_signal_handler(&self, call: &dyn Fn(&SignalHandler) -> bool) -> bool;
}

enum UnwindReason {
    /// A panic caused by the host
    Panic(Box<dyn Any + Send>),
    /// A custom error triggered by the user
    UserTrap(Box<dyn Error + Send + Sync>),
    /// A Trap triggered by a wasm libcall
    LibTrap(Trap),
    /// A trap caused by the Wasm generated code
    WasmTrap { backtrace: Backtrace, pc: usize },
}

impl<'a> CallThreadState<'a> {
    #[inline]
    fn new(trap_info: &'a (dyn TrapInfo + 'a)) -> CallThreadState<'a> {
        CallThreadState {
            unwind: UnsafeCell::new(MaybeUninit::uninit()),
            jmp_buf: Cell::new(ptr::null()),
            handling_trap: Cell::new(false),
            trap_info,
            prev: Cell::new(ptr::null()),
        }
    }

    fn with(self, closure: impl FnOnce(&CallThreadState) -> i32) -> Result<(), Trap> {
        let ret = tls::set(&self, || closure(&self));
        println!("with: ret:{}", ret);
        if ret != 0 {
            return Ok(());
        }
        match unsafe { (*self.unwind.get()).as_ptr().read() } {
            UnwindReason::UserTrap(data) => Err(Trap::User(data)),
            UnwindReason::LibTrap(trap) => Err(trap),
            UnwindReason::WasmTrap { backtrace, pc } => Err(Trap::Wasm {
                pc,
                backtrace,
                signal_trap: None,
            }),
            UnwindReason::Panic(panic) => std::panic::resume_unwind(panic),
        }
    }

    /// Trap handler using our thread-local state.
    ///
    /// * `pc` - the program counter the trap happened at
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
    #[cfg_attr(target_os = "macos", allow(dead_code))] // macOS is more raw and doesn't use this
    fn jmp_buf_if_trap(
        &self,
        pc: *const u8,
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
        let _reset = ResetCell(&self.handling_trap, false);

        // If we haven't even started to handle traps yet, bail out.
        if self.jmp_buf.get().is_null() {
            return ptr::null();
        }

        // First up see if any instance registered has a custom trap handler,
        // in which case run them all. If anything handles the trap then we
        // return that the trap was handled.
        if self.trap_info.custom_signal_handler(&call_handler) {
            return 1 as *const _;
        }

        // If this fault wasn't in wasm code, then it's not our problem
        if unsafe { !IS_WASM_PC(pc as usize) } {
            return ptr::null();
        }

        // If all that passed then this is indeed a wasm trap, so return the
        // `jmp_buf` passed to `Unwind` to resume.
        self.jmp_buf.get()
    }

    fn unwind_with(&self, reason: UnwindReason) -> ! {
        println!("Unwind with");
        unsafe {
            (*self.unwind.get()).as_mut_ptr().write(reason);
            unwind(self.jmp_buf.get());
        }
    }

    fn capture_backtrace(&self, pc: *const u8) {
        println!("TRAP::capture_backtrace 0");
        let backtrace = Backtrace::new_unresolved();
        println!("TRAP::capture_backtrace 1");
        unsafe {
            (*self.unwind.get())
                .as_mut_ptr()
                .write(UnwindReason::WasmTrap {
                    backtrace,
                    pc: pc as usize,
                });
        }
        println!("end capturing");
    }
}

struct ResetCell<'a, T: Copy>(&'a Cell<T>, T);

impl<T: Copy> Drop for ResetCell<'_, T> {
    #[inline]
    fn drop(&mut self) {
        self.0.set(self.1);
    }
}

// A private inner module for managing the TLS state that we require across
// calls in wasm. The WebAssembly code is called from C++ and then a trap may
// happen which requires us to read some contextual state to figure out what to
// do with the trap. This `tls` module is used to persist that information from
// the caller to the trap site.
mod tls {
    use super::CallThreadState;
    use std::mem;
    use std::ptr;

    pub use raw::Ptr;

    // An even *more* inner module for dealing with TLS. This actually has the
    // thread local variable and has functions to access the variable.
    //
    // Note that this is specially done to fully encapsulate that the accessors
    // for tls must not be inlined. Wasmer's (not yet implemented async support
    // will employ stack switching which can resume execution on different OS threads.
    //
    // This means that borrows of our TLS pointer must never live across accesses because
    // otherwise the access may be split across two threads and cause unsafety.
    //
    // This also means that extra care is taken by the runtime to save/restore
    // these TLS values when the runtime may have crossed threads.
    mod raw {
        use super::CallThreadState;
        use std::cell::Cell;
        use std::ptr;

        pub type Ptr = *const CallThreadState<'static>;

        thread_local!(static PTR: Cell<Ptr> = Cell::new(ptr::null()));

        #[inline(never)] // see module docs for why this is here
        pub fn replace(val: Ptr) -> Ptr {
            PTR.with(|p| p.replace(val))
        }

        #[inline(never)] // see module docs for why this is here
        pub fn get() -> Ptr {
            PTR.with(|p| p.get())
        }
    }

    /// Opaque state used to help control TLS state across stack switches for
    /// async support.
    pub struct TlsRestore(raw::Ptr);

    impl TlsRestore {
        /// Takes the TLS state that is currently configured and returns a
        /// token that is used to replace it later.
        ///
        /// This is unsafe because it's intended to only be used within the
        /// context of stack switching.
        pub unsafe fn take() -> TlsRestore {
            // Our tls pointer must be set at this time, and it must not be
            // null. We need to restore the previous pointer since we're
            // removing ourselves from the call-stack, and in the process we
            // null out our own previous field for safety in case it's
            // accidentally used later.
            let raw = raw::get();
            assert!(!raw.is_null());
            let prev = (*raw).prev.replace(ptr::null());
            raw::replace(prev);
            TlsRestore(raw)
        }

        /// Restores a previous tls state back into this thread's TLS.
        ///
        /// This is unsafe because it's intended to only be used within the
        /// context of stack switching.
        pub unsafe fn replace(self) -> Result<(), super::Trap> {
            // When replacing to the previous value of TLS, we might have
            // crossed a thread: make sure the trap-handling lazy initializer
            // runs.
            super::sys::lazy_per_thread_init()?;

            // We need to configure our previous TLS pointer to whatever is in
            // TLS at this time, and then we set the current state to ourselves.
            let prev = raw::get();
            assert!((*self.0).prev.get().is_null());
            (*self.0).prev.set(prev);
            raw::replace(self.0);
            Ok(())
        }
    }

    /// Configures thread local state such that for the duration of the
    /// execution of `closure` any call to `with` will yield `ptr`, unless this
    /// is recursively called again.
    pub fn set<R>(state: &CallThreadState<'_>, closure: impl FnOnce() -> R) -> R {
        struct Reset<'a, 'b>(&'a CallThreadState<'b>);

        impl Drop for Reset<'_, '_> {
            #[inline]
            fn drop(&mut self) {
                raw::replace(self.0.prev.replace(ptr::null()));
            }
        }

        // Note that this extension of the lifetime to `'static` should be
        // safe because we only ever access it below with an anonymous
        // lifetime, meaning `'static` never leaks out of this module.
        let ptr = unsafe {
            mem::transmute::<*const CallThreadState<'_>, *const CallThreadState<'static>>(state)
        };
        let prev = raw::replace(ptr);
        state.prev.set(prev);
        let _reset = Reset(state);
        closure()
    }

    /// Returns the last pointer configured with `set` above. Panics if `set`
    /// has not been previously called.
    pub fn with<R>(closure: impl FnOnce(Option<&CallThreadState<'_>>) -> R) -> R {
        let p = raw::get();
        unsafe { closure(if p.is_null() { None } else { Some(&*p) }) }
    }
}
