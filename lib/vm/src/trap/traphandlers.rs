// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

#![allow(static_mut_refs)]

//! WebAssembly trap handling, which is built on top of the lower-level
//! signalhandling mechanisms.

use super::trap::UnwindReason;
use crate::Trap;
#[cfg(feature = "experimental-host-interrupt")]
use crate::interrupt_registry;
use backtrace::Backtrace;
use bytesize::ByteSize;
use core::ptr::{read, read_unaligned};
use corosensei::stack::{DefaultStack, Stack};
use corosensei::trap::{CoroutineTrapHandler, TrapHandlerRegs};
use corosensei::{Coroutine, CoroutineResult, Yielder};
use scopeguard::defer;
use std::any::Any;
use std::cell::{Cell, UnsafeCell};
use std::error::Error;
use std::io;
use std::mem::MaybeUninit;
use std::mem::{self, ManuallyDrop};
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

/// How much of the Wasm stack must be left for a synchronous host function
/// called from Wasm to run on it, in bytes: 256 KiB.
///
/// A host function called from Wasm runs on the Wasm stack it was called
/// from when at least this much of that stack is left, which saves switching
/// back to the native stack and back again. With less left it runs on the
/// native stack. The reserve is all the stack a host function is sure to have
/// there: one that may need more must be made to run on the native stack
/// (`Function::set_calls_on_native_stack` in the API). A host function that
/// overflows the Wasm stack aborts the process, as overflowing the native
/// stack does.
///
/// The space is measured from the lowest address of the Wasm stack, above
/// its guard page.
pub const HOST_CALL_RESERVE: usize = ByteSize::kib(256).as_u64() as usize;

/// Pool of pre-allocated coroutine stacks to avoid repeated mmap syscalls.
/// Acts as the cross-thread overflow store; per-thread reuse is served by
/// `ThreadState::stack` to keep the hot path atomic-free.
static STACK_POOL: LazyLock<crossbeam_queue::SegQueue<DefaultStack>> =
    LazyLock::new(crossbeam_queue::SegQueue::new);

/// The per-thread state of the trap machinery.
///
/// Every entry into Wasm and every call out of it to the host touches this
/// state, so it lives in a single `const` thread-local without a destructor:
/// reaching it is a plain thread-local address lookup, with no lazy
/// initialization or destructor-registration check. An entry looks it up once
/// and passes the reference down instead of going through one thread-local
/// per field. The one-time per-thread work (the signal stack, returning the
/// cached stacks at thread exit) is tracked with bits in `flags` and done out
/// of line the first time it is needed.
///
/// On targets where the standard library keeps thread-locals behind OS keys
/// rather than native thread-local storage, every thread-local is a heap block
/// that is freed when the thread exits, destructor or not, so there
/// `ThreadState` has a destructor of its own that returns the cached stacks
/// (see its `Drop` impl).
struct ThreadState {
    /// The trap handler context of the innermost Wasm entry on this thread.
    /// It is set from outside the Wasm stack and read only by signal
    /// handlers, so it is atomic. It belongs to the running activation and is
    /// exchanged by [`ActivationState::swap`].
    trap_handler: AtomicPtr<TrapHandlerContext>,
    /// The Wasm stack the thread is running Wasm on (see `WasmStack`). It is
    /// set on that stack at the start of each call, used to unwind back to
    /// the root of the stack from inside it and to decide whether a host
    /// function may run on it, and taken while host code runs, on whichever
    /// stack: `None` means no Wasm is running at this point, which the signal
    /// handler relies on to tell faults in host code from Wasm traps.
    wasm_stack: Cell<Option<NonNull<WasmStack>>>,
    /// Entry coroutines kept between calls, suspended (see `EntryCoroutine`):
    /// a call is one resume of one of them instead of setting up a new
    /// coroutine. A call takes the first slot no running call uses and uses
    /// it in place; one that finds them all in use makes a coroutine of its
    /// own. Slot 0 serves calls from the host and slot 1 calls back into
    /// Wasm from a host function. Slots are not tied to a nesting depth: the
    /// activations of `Function::call_async` can finish in any order.
    ///
    /// `ManuallyDrop` for the same reason as `stack`: at thread exit the
    /// cached coroutines are turned back into stacks for `STACK_POOL`.
    entries: ManuallyDrop<[UnsafeCell<Option<EntryCoroutine>>; ENTRY_SLOTS]>,
    /// Bit `i` set: a running call uses `entries[i]`.
    entry_busy: Cell<u8>,
    /// The call an entry coroutine runs when it is next resumed. It is set
    /// right before each resume and passed here rather than as the resume
    /// value, which then stays a word (see `EntryYielder`).
    job: Cell<EntryJob>,
    /// A single ready-to-use coroutine stack, for making entry coroutines
    /// without touching the global `STACK_POOL`'s atomics. An entry coroutine
    /// that is not kept (after a trap, or one made for a nested call) gives
    /// its stack back here.
    ///
    /// `ManuallyDrop` keeps the thread-local free of a destructor. On thread
    /// exit `ThreadExit` pushes the held stack to `STACK_POOL` (or, where the
    /// state itself is freed at exit, `ThreadState`'s destructor does), so
    /// memory cycles correctly across thread lifetimes (no mmap leaks).
    stack: ManuallyDrop<Cell<Option<DefaultStack>>>,
    /// One-time per-thread work already done: `SIGNAL_STACK_READY`,
    /// `EXIT_HOOK_REGISTERED`.
    flags: Cell<u8>,
    /// See [`CurrentThread::slot_call`]. It lives here so that an embedder
    /// that sets it around an entry into Wasm looks up one thread-local for
    /// both.
    slot_call: Cell<bool>,
    /// See [`CurrentThread::cothread`]; here for the same reason.
    cothread: Cell<bool>,
}

/// How many entry coroutines a thread keeps: one for calls from the host and
/// one for calls back into Wasm from a host function. Deeper nesting makes a
/// coroutine per call.
const ENTRY_SLOTS: usize = 2;

/// `lazy_per_thread_init` has set up this thread's signal stack.
#[cfg(unix)]
const SIGNAL_STACK_READY: u8 = 1 << 0;
/// `ThreadExit` is registered to return the cached stacks when the thread
/// exits.
const EXIT_HOOK_REGISTERED: u8 = 1 << 1;

impl ThreadState {
    fn set_flag(&self, flag: u8) {
        self.flags.set(self.flags.get() | flag);
    }

    fn clear_flag(&self, flag: u8) {
        self.flags.set(self.flags.get() & !flag);
    }

    /// Ensures the per-thread setup that has to happen before any WebAssembly
    /// is entered on this thread.
    #[inline]
    fn per_thread_init(&self) -> Result<(), Trap> {
        #[cfg(unix)]
        if self.flags.get() & SIGNAL_STACK_READY == 0 {
            return lazy_per_thread_init(self);
        }
        #[cfg(windows)]
        lazy_per_thread_init()?;
        Ok(())
    }

    /// Takes the cached entry coroutines that no running call uses, as their
    /// stacks.
    fn take_idle_entries(&self) -> impl Iterator<Item = DefaultStack> + '_ {
        (0..ENTRY_SLOTS).filter_map(move |i| {
            if self.entry_busy.get() & (1 << i) != 0 {
                return None;
            }
            // SAFETY: not busy, so no call holds a reference into the slot.
            unsafe { (*self.entries[i].get()).take() }.map(retire_entry)
        })
    }

    /// Registers `ThreadExit` for this thread. Returns false if the thread is
    /// already past that point of its exit, when nothing can be registered.
    #[cold]
    #[inline(never)]
    fn register_exit_hook(&self) -> bool {
        let registered = THREAD_EXIT.try_with(|_| ()).is_ok();
        if registered {
            self.set_flag(EXIT_HOOK_REGISTERED);
        }
        registered
    }
}

// Targets whose thread-locals are heap blocks behind OS keys rather than
// native thread-local storage (Android, OpenBSD, windows-gnu, and a few more):
// the standard library frees each of them when the thread exits, whether or
// not the value has a destructor. There `ThreadState` has one, which returns
// the cached stack: `ThreadExit` may run after the block was freed (the
// destructors of a thread's keys run in an order of the platform's choosing,
// oldest first on Android), and would then find no stack to return. A target
// listed here that has native thread-locals only pays for a destructor it does
// not need; one that is missing leaks the cached stack of every thread that
// exits. The list is repeated in the check below.
#[cfg(any(
    target_os = "android",
    target_os = "openbsd",
    target_os = "illumos",
    target_os = "solaris",
    target_os = "haiku",
    target_os = "nto",
    target_env = "ohos",
    all(windows, target_env = "gnu"),
))]
impl Drop for ThreadState {
    fn drop(&mut self) {
        for stack in self.take_idle_entries() {
            STACK_POOL.push(stack);
        }
        if let Some(stack) = self.stack.take() {
            STACK_POOL.push(stack);
        }
    }
}

// Everywhere else `ThreadState` must not need dropping: a field with a
// destructor would make the `const` thread-local register one, and every
// access check whether it ran.
#[cfg(not(any(
    target_os = "android",
    target_os = "openbsd",
    target_os = "illumos",
    target_os = "solaris",
    target_os = "haiku",
    target_os = "nto",
    target_env = "ohos",
    all(windows, target_env = "gnu"),
)))]
const _: () = assert!(!std::mem::needs_drop::<ThreadState>());

thread_local! {
    static THREAD_STATE: ThreadState = const {
        ThreadState {
            trap_handler: AtomicPtr::new(ptr::null_mut()),
            wasm_stack: Cell::new(None),
            entries: ManuallyDrop::new([UnsafeCell::new(None), UnsafeCell::new(None)]),
            entry_busy: Cell::new(0),
            job: Cell::new(EntryJob::NONE),
            stack: ManuallyDrop::new(Cell::new(None)),
            flags: Cell::new(0),
            slot_call: Cell::new(false),
            cothread: Cell::new(false),
        }
    };
}

/// The calling thread's state for entering Wasm, looked up once.
///
/// Every entry into Wasm looks up the state of its thread, a thread-local. An
/// embedder that keeps a mark of its own on the thread around each entry (see
/// [`Self::slot_call`]) can look the thread up once for both: it gets the
/// thread, sets its mark, and enters with
/// [`wasmer_call_trampoline_on`](crate::wasmer_call_trampoline_on).
///
/// It is not `Send`: it belongs to the thread that got it.
#[derive(Clone, Copy)]
pub struct CurrentThread {
    state: NonNull<ThreadState>,
}

impl CurrentThread {
    /// The calling thread's.
    #[inline]
    pub fn get() -> Self {
        Self {
            state: THREAD_STATE.with(|state| NonNull::from(state)),
        }
    }

    /// [`Self::get`] out of line, for a fallback: inlined, the lookup may be
    /// done ahead of the branch that needs it, as the compiler is free to.
    #[cold]
    #[inline(never)]
    pub fn get_out_of_line() -> Self {
        Self::get()
    }

    /// The calling thread's, or `None` on targets where thread-locals are
    /// freed when the thread exits, once its state was.
    #[inline]
    pub fn try_get() -> Option<Self> {
        THREAD_STATE
            .try_with(|state| Self {
                state: NonNull::from(state),
            })
            .ok()
    }

    #[inline]
    fn state(&self) -> &ThreadState {
        // SAFETY: as in `with_thread_state`; a `CurrentThread` stays on its
        // thread.
        unsafe { self.state.as_ref() }
    }

    /// Whether the innermost call into a store running on this thread went
    /// through the store's slot (see
    /// [`StoreObjects::slot`](crate::StoreObjects::slot)).
    ///
    /// The VM only keeps it: the embedder sets it for the duration of such a
    /// call, puts back the previous value when the call returns, and reads it
    /// from host code.
    #[inline]
    pub fn slot_call(&self) -> &Cell<bool> {
        &self.state().slot_call
    }

    /// Whether the thread runs a coroutine that the embedder switches itself,
    /// whose calls into stores keep the state they leave on the thread where
    /// the embedder can take it with the coroutine when it suspends.
    ///
    /// The VM only keeps it: the embedder sets it while such a coroutine
    /// runs, and reads it when a call enters a store.
    #[inline]
    pub fn cothread(&self) -> &Cell<bool> {
        &self.state().cothread
    }
}

/// Runs `f` with the calling thread's state.
///
/// Same as `THREAD_STATE.with(f)`, except that the thread-local is looked up
/// by a closure small enough to be inlined: when `f` is large, `with` itself
/// is not inlined, and then the lookup becomes an indirect call through the
/// `LocalKey`.
#[inline(always)]
fn with_thread_state<R>(f: impl FnOnce(&ThreadState) -> R) -> R {
    let state = THREAD_STATE.with(ptr::from_ref);
    // SAFETY: with native thread-locals `THREAD_STATE` has no destructor, so
    // it lives as long as the thread, and `f` runs on this thread, within this
    // call. Where thread-locals are freed at exit (see `ThreadState`), it
    // lives until the thread's thread-local destructors run.
    f(unsafe { &*state })
}

/// Returns the cached stacks to the global pool when the thread exits. This
/// is a thread-local of its own, accessed once per thread, so that the
/// destructor's bookkeeping stays off `THREAD_STATE`, which every call
/// touches.
struct ThreadExit;

impl Drop for ThreadExit {
    fn drop(&mut self) {
        // With native thread-locals `THREAD_STATE` has no destructor, so it
        // is still there. Where thread-locals are freed at exit it may be gone
        // already, having returned its stack itself; looking it up then makes
        // a new, empty one, which has nothing to return.
        let _ = THREAD_STATE.try_with(|state| {
            state.clear_flag(EXIT_HOOK_REGISTERED);
            for stack in state.take_idle_entries() {
                STACK_POOL.push(stack);
            }
            if let Some(stack) = state.stack.take() {
                STACK_POOL.push(stack);
            }
        });
    }
}

thread_local! {
    static THREAD_EXIT: ThreadExit = const { ThreadExit };
}

/// Acquire a coroutine stack large enough for `min_size`. Prefers the
/// thread-local cache (no atomics), falls back to the global `STACK_POOL`,
/// then allocates a fresh stack.
fn acquire_stack(state: &ThreadState, min_size: usize) -> DefaultStack {
    // Fast path: thread-local cache. Steady-state per-thread reuse never
    // touches the SegQueue.
    if let Some(stack) = state.stack.take() {
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
fn release_stack(state: &ThreadState, stack: DefaultStack) {
    // A stack may only be cached once something will return it to the pool
    // when the thread exits.
    if state.flags.get() & EXIT_HOOK_REGISTERED == 0 && !state.register_exit_hook() {
        STACK_POOL.push(stack);
        return;
    }
    let displaced = state.stack.replace(Some(stack));
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
/// thread also keeps private cached stacks in TLS (a spare one, and those of
/// its entry coroutines). Other threads may push
/// stacks back into the pool (for example, when their Wasm execution
/// finishes) while or after this function is running, and TLS-cached stacks
/// on other threads are not touched. As a result, this function provides
/// only a best-effort drain: there is no guarantee that no undersized stacks
/// exist immediately after it returns unless the caller ensures, via external
/// synchronization, that no other Wasm executions can return stacks to the
/// pool while this function runs. The current thread's TLS-cached stacks are
/// drained as part of this call, except those of entry coroutines a running
/// call uses.
pub fn drain_stack_pool() {
    // Drain the calling thread's TLS slot first (best-effort across threads
    // still applies — other threads' caches aren't touched).
    THREAD_STATE.with(|state| state.take_idle_entries().for_each(drop));
    if let Some(stack) = THREAD_STATE.with(|state| state.stack.take()) {
        drop(stack);
    }
    while STACK_POOL.pop().is_some() {}
}

cfg_select! {
    unix => {
        /// Function which may handle custom signals while processing traps.
        pub type TrapHandlerFn<'a> = dyn Fn(libc::c_int, *const libc::siginfo_t, *const libc::c_void) -> bool + Send + Sync + 'a;
    }
    target_os = "windows" => {
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

cfg_select! {
    unix => {
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
            cfg_select! {
                all(
                    any(target_os = "linux", target_os = "android"),
                    target_arch = "x86_64",
                ) => {
                    pc = context.uc_mcontext.gregs[libc::REG_RIP as usize] as usize;
                    sp = context.uc_mcontext.gregs[libc::REG_RSP as usize] as usize;
                }
                all(
                    any(target_os = "linux", target_os = "android"),
                    target_arch = "x86",
                ) => {
                    pc = context.uc_mcontext.gregs[libc::REG_EIP as usize] as usize;
                    sp = context.uc_mcontext.gregs[libc::REG_ESP as usize] as usize;
                }
                all(target_os = "freebsd", target_arch = "x86") => {
                    pc = context.uc_mcontext.mc_eip as usize;
                    sp = context.uc_mcontext.mc_esp as usize;
                }
                all(target_os = "freebsd", target_arch = "x86_64") => {
                    pc = context.uc_mcontext.mc_rip as usize;
                    sp = context.uc_mcontext.mc_rsp as usize;
                }
                all(target_vendor = "apple", target_arch = "x86_64") => {
                    let mcontext = unsafe { &*context.uc_mcontext };
                    pc = mcontext.__ss.__rip as usize;
                    sp = mcontext.__ss.__rsp as usize;
                }
                all(
                        any(target_os = "linux", target_os = "android"),
                        target_arch = "aarch64",
                    ) => {
                    pc = context.uc_mcontext.pc as usize;
                    sp = context.uc_mcontext.sp as usize;
                }
                all(
                    any(target_os = "linux", target_os = "android"),
                    target_arch = "arm",
                ) => {
                    pc = context.uc_mcontext.arm_pc as usize;
                    sp = context.uc_mcontext.arm_sp as usize;
                }
                all(
                    any(target_os = "linux", target_os = "android"),
                    any(target_arch = "riscv64", target_arch = "riscv32"),
                ) => {
                    pc = context.uc_mcontext.__gregs[libc::REG_PC] as usize;
                    sp = context.uc_mcontext.__gregs[libc::REG_SP] as usize;
                }
                all(target_vendor = "apple", target_arch = "aarch64") => {
                    let mcontext = unsafe { &*context.uc_mcontext };
                    pc = mcontext.__ss.__pc as usize;
                    sp = mcontext.__ss.__sp as usize;
                }
                all(target_os = "freebsd", target_arch = "aarch64") => {
                    pc = context.uc_mcontext.mc_gpregs.gp_elr as usize;
                    sp = context.uc_mcontext.mc_gpregs.gp_sp as usize;
                }
                all(target_os = "linux", target_arch = "loongarch64") => {
                    pc = context.uc_mcontext.__gregs[1] as usize;
                    sp = context.uc_mcontext.__gregs[3] as usize;
                }
                all(target_os = "linux", target_arch = "powerpc64") => {
                    pc = (*context.uc_mcontext.regs).nip as usize;
                    sp = (*context.uc_mcontext.regs).gpr[1] as usize;
                }
                _ => {
                    compile_error!("Unsupported platform");
                }
            };
            (pc, sp)
        }

        unsafe fn update_context(context: &mut ucontext_t, regs: TrapHandlerRegs) {
            cfg_select! {
                all(
                        any(target_os = "linux", target_os = "android"),
                        target_arch = "x86_64",
                    ) => {
                    let TrapHandlerRegs { rip, rsp, rbp, rdi, rsi } = regs;
                    context.uc_mcontext.gregs[libc::REG_RIP as usize] = rip as i64;
                    context.uc_mcontext.gregs[libc::REG_RSP as usize] = rsp as i64;
                    context.uc_mcontext.gregs[libc::REG_RBP as usize] = rbp as i64;
                    context.uc_mcontext.gregs[libc::REG_RDI as usize] = rdi as i64;
                    context.uc_mcontext.gregs[libc::REG_RSI as usize] = rsi as i64;
                }
                all(
                    any(target_os = "linux", target_os = "android"),
                    target_arch = "x86",
                ) => {
                    let TrapHandlerRegs { eip, esp, ebp, ecx, edx } = regs;
                    context.uc_mcontext.gregs[libc::REG_EIP as usize] = eip as i32;
                    context.uc_mcontext.gregs[libc::REG_ESP as usize] = esp as i32;
                    context.uc_mcontext.gregs[libc::REG_EBP as usize] = ebp as i32;
                    context.uc_mcontext.gregs[libc::REG_ECX as usize] = ecx as i32;
                    context.uc_mcontext.gregs[libc::REG_EDX as usize] = edx as i32;
                }
                all(target_vendor = "apple", target_arch = "x86_64") => {
                    let TrapHandlerRegs { rip, rsp, rbp, rdi, rsi } = regs;
                    let mcontext = unsafe { &mut *context.uc_mcontext };
                    mcontext.__ss.__rip = rip;
                    mcontext.__ss.__rsp = rsp;
                    mcontext.__ss.__rbp = rbp;
                    mcontext.__ss.__rdi = rdi;
                    mcontext.__ss.__rsi = rsi;
                }
                all(target_os = "freebsd", target_arch = "x86") => {
                    let TrapHandlerRegs { eip, esp, ebp, ecx, edx } = regs;
                    context.uc_mcontext.mc_eip = eip as libc::register_t;
                    context.uc_mcontext.mc_esp = esp as libc::register_t;
                    context.uc_mcontext.mc_ebp = ebp as libc::register_t;
                    context.uc_mcontext.mc_ecx = ecx as libc::register_t;
                    context.uc_mcontext.mc_edx = edx as libc::register_t;
                }
                all(target_os = "freebsd", target_arch = "x86_64") => {
                    let TrapHandlerRegs { rip, rsp, rbp, rdi, rsi } = regs;
                    context.uc_mcontext.mc_rip = rip as libc::register_t;
                    context.uc_mcontext.mc_rsp = rsp as libc::register_t;
                    context.uc_mcontext.mc_rbp = rbp as libc::register_t;
                    context.uc_mcontext.mc_rdi = rdi as libc::register_t;
                    context.uc_mcontext.mc_rsi = rsi as libc::register_t;
                }
                all(
                        any(target_os = "linux", target_os = "android"),
                        target_arch = "aarch64",
                    ) => {
                    let TrapHandlerRegs { pc, sp, x0, x1, x29, lr } = regs;
                    context.uc_mcontext.pc = pc;
                    context.uc_mcontext.sp = sp;
                    context.uc_mcontext.regs[0] = x0;
                    context.uc_mcontext.regs[1] = x1;
                    context.uc_mcontext.regs[29] = x29;
                    context.uc_mcontext.regs[30] = lr;
                }
                all(
                        any(target_os = "linux", target_os = "android"),
                        target_arch = "arm",
                    ) => {
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
                }
                all(
                    any(target_os = "linux", target_os = "android"),
                    any(target_arch = "riscv64", target_arch = "riscv32"),
                ) => {
                    let TrapHandlerRegs { pc, ra, sp, a0, a1, s0 } = regs;
                    context.uc_mcontext.__gregs[libc::REG_PC] = pc as libc::c_ulong;
                    context.uc_mcontext.__gregs[libc::REG_RA] = ra as libc::c_ulong;
                    context.uc_mcontext.__gregs[libc::REG_SP] = sp as libc::c_ulong;
                    context.uc_mcontext.__gregs[libc::REG_A0] = a0 as libc::c_ulong;
                    context.uc_mcontext.__gregs[libc::REG_A0 + 1] = a1 as libc::c_ulong;
                    context.uc_mcontext.__gregs[libc::REG_S0] = s0 as libc::c_ulong;
                }
                all(target_vendor = "apple", target_arch = "aarch64") => {
                    let TrapHandlerRegs { pc, sp, x0, x1, x29, lr } = regs;
                    let mcontext = unsafe { &mut *context.uc_mcontext };
                    mcontext.__ss.__pc = pc;
                    mcontext.__ss.__sp = sp;
                    mcontext.__ss.__x[0] = x0;
                    mcontext.__ss.__x[1] = x1;
                    mcontext.__ss.__fp = x29;
                    mcontext.__ss.__lr = lr;
                }
                all(target_os = "freebsd", target_arch = "aarch64") => {
                    let TrapHandlerRegs { pc, sp, x0, x1, x29, lr } = regs;
                    context.uc_mcontext.mc_gpregs.gp_elr = pc as libc::register_t;
                    context.uc_mcontext.mc_gpregs.gp_sp = sp as libc::register_t;
                    context.uc_mcontext.mc_gpregs.gp_x[0] = x0 as libc::register_t;
                    context.uc_mcontext.mc_gpregs.gp_x[1] = x1 as libc::register_t;
                    context.uc_mcontext.mc_gpregs.gp_x[29] = x29 as libc::register_t;
                    context.uc_mcontext.mc_gpregs.gp_lr = lr as libc::register_t;
                }
                all(target_os = "linux", target_arch = "loongarch64") => {
                    let TrapHandlerRegs { pc, sp, a0, a1, fp, ra } = regs;
                    context.uc_mcontext.__pc = pc;
                    context.uc_mcontext.__gregs[1] = ra;
                    context.uc_mcontext.__gregs[3] = sp;
                    context.uc_mcontext.__gregs[4] = a0;
                    context.uc_mcontext.__gregs[5] = a1;
                    context.uc_mcontext.__gregs[22] = fp;
                }
                all(target_os = "linux", target_arch = "powerpc64") => {
                    let TrapHandlerRegs { pc, sp, r3, r4, r31, lr } = regs;
                    (*context.uc_mcontext.regs).nip = pc;
                    (*context.uc_mcontext.regs).gpr[1] = sp;
                    (*context.uc_mcontext.regs).gpr[3] = r3;
                    (*context.uc_mcontext.regs).gpr[4] = r4;
                    (*context.uc_mcontext.regs).gpr[31] = r31;
                    (*context.uc_mcontext.regs).link = lr;
                }
                _ => {
                    compile_error!("Unsupported platform");
                }
            };
        }
    }
    target_os = "windows" => {
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
            cfg_select! {
                target_arch = "x86_64" => {
                    pc = context.Rip as usize;
                    sp = context.Rsp as usize;
                }
                target_arch = "x86" => {
                    pc = context.Eip as usize;
                    sp = context.Esp as usize;
                }
                _ => {
                    compile_error!("Unsupported platform");
                }
            };
            (pc, sp)
        }

        unsafe fn update_context(context: &mut CONTEXT, regs: TrapHandlerRegs) {
            cfg_select! {
                target_arch = "x86_64" => {
                    let TrapHandlerRegs { rip, rsp, rbp, rdi, rsi } = regs;
                    context.Rip = rip;
                    context.Rsp = rsp;
                    context.Rbp = rbp;
                    context.Rdi = rdi;
                    context.Rsi = rsi;
                }
                target_arch = "x86" => {
                    let TrapHandlerRegs { eip, esp, ebp, ecx, edx } = regs;
                    context.Eip = eip;
                    context.Esp = esp;
                    context.Ebp = ebp;
                    context.Ecx = ecx;
                    context.Edx = edx;
                }
                _ => {
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
    // Miri cannot call `sigaction`/`sigemptyset`, and nothing under Miri
    // executes compiled Wasm, so there is no trap for a handler to catch.
    // Without this, building a `Store` at all is out of reach there, and the
    // store-context tests that need one cannot run.
    if cfg!(miri) {
        return;
    }
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

/// Catches any wasm traps that happen within the execution of `closure`,
/// returning them as a `Result`.
///
/// # Safety
///
/// Highly unsafe since `closure` won't have any dtors run.
#[inline]
pub unsafe fn catch_traps<F, R: 'static>(
    trap_handler: Option<*const TrapHandlerFn<'static>>,
    config: &VMConfig,
    closure: F,
) -> Result<R, Trap>
where
    F: FnOnce() -> R + 'static,
{
    unsafe { catch_traps_on(CurrentThread::get(), trap_handler, config, closure) }
}

/// [`catch_traps`] on `thread`, which must be the calling thread's.
///
/// # Safety
///
/// See [`catch_traps`].
#[inline]
pub(crate) unsafe fn catch_traps_on<F, R: 'static>(
    thread: CurrentThread,
    trap_handler: Option<*const TrapHandlerFn<'static>>,
    config: &VMConfig,
    closure: F,
) -> Result<R, Trap>
where
    F: FnOnce() -> R + 'static,
{
    // One thread-local lookup for the whole entry: the call stays on this
    // thread until it returns, so the reference stays valid throughout.
    let state = thread.state();
    // Ensure that per-thread initialization is done.
    state.per_thread_init()?;
    let stack_size = config
        .wasm_stack_size
        .unwrap_or_else(|| DEFAULT_STACK_SIZE.load(Ordering::Relaxed));
    on_wasm_stack(state, stack_size, trap_handler, closure).map_err(UnwindReason::into_trap)
}

/// The thread-local state of one *activation*: a chain of Wasm entries whose
/// frames all live on one host stack. It is what the signal handler consults
/// to find the Wasm code that is running: the trap handler and, with
/// `experimental-host-interrupt`, the interrupt registry's active stores.
///
/// A thread that runs all its Wasm calls on one host stack never needs this
/// type: its calls nest, and each entry restores what it found. It is for
/// code that keeps several host stacks on one thread and switches between
/// them while they have Wasm frames on them, as `Function::call_async` in the
/// API crate does with one coroutine per call. Suspended activations can be
/// resumed in any order, so the state has to follow the stack that is
/// running: an activation that is not running keeps its state here, and
/// [`Self::swap`] exchanges it with the thread's at every switch, in both
/// directions. An embedder that switches such stacks itself has to do the
/// same around its own switches.
pub struct ActivationState {
    trap_handler: *mut TrapHandlerContext,
    #[cfg(feature = "experimental-host-interrupt")]
    active_stores: interrupt_registry::ActiveStores,
}

impl Default for ActivationState {
    fn default() -> Self {
        Self::new()
    }
}

impl ActivationState {
    /// The state of an activation that has not entered Wasm.
    pub const fn new() -> Self {
        Self {
            trap_handler: ptr::null_mut(),
            #[cfg(feature = "experimental-host-interrupt")]
            active_stores: interrupt_registry::ActiveStores::new(),
        }
    }

    /// Exchanges this state with the calling thread's.
    ///
    /// # Safety
    ///
    /// The state holds pointers into the host stack its activation runs on,
    /// and the signal handler follows whichever of them the thread holds.
    ///
    /// - Calls come in pairs on one thread: one immediately before switching
    ///   to the stack that owns this state, and one immediately after control
    ///   comes back from it, whether it suspended, returned or unwound.
    /// - It is called from host code, never while running on a Wasm stack
    ///   (host functions run inside [`on_host_stack`], so they qualify).
    /// - A state that is not empty (see [`Self::is_empty`]) must not be
    ///   swapped in once the stack that owns it is gone, nor on another
    ///   thread.
    pub unsafe fn swap(&mut self) {
        // Only the trap handler (and the interrupt registry's active stores)
        // belongs to an activation. The Wasm stack is not part of the
        // exchange because it is unset on both sides: host code runs with it
        // taken (`on_host_stack`, `host_call_in_place`), and `on_wasm_stack`
        // clears it on the way out. The cached stacks, the entry coroutines
        // and the one-time setup flags belong to the thread.
        THREAD_STATE.with(|state| {
            debug_assert!(
                state.wasm_stack.get().is_none(),
                "activation switched while running on the Wasm stack"
            );
            compiler_fence(Ordering::Release);
            self.trap_handler = state
                .trap_handler
                .swap(self.trap_handler, Ordering::Relaxed);
            compiler_fence(Ordering::Acquire);
        });
        #[cfg(feature = "experimental-host-interrupt")]
        self.active_stores.swap();
    }

    /// Whether this activation, while it is not running, has entered
    /// `store_id` and the store was interrupted.
    #[cfg(feature = "experimental-host-interrupt")]
    pub fn is_interrupted(&self, store_id: wasmer_types::StoreId) -> bool {
        self.active_stores.is_interrupted(store_id)
    }

    /// Whether the calling thread still has what swapping a state in, and
    /// unwinding a stack suspended in Wasm, need of it: false once its thread
    /// locals are being destroyed, as it exits. A stack suspended then is to
    /// be forgotten rather than unwound.
    pub fn thread_is_usable() -> bool {
        // `ThreadExit` is the thread-local of this module with a destructor:
        // once that ran, the thread is exiting.
        let usable = THREAD_EXIT.try_with(|_| ()).is_ok() && THREAD_STATE.try_with(|_| ()).is_ok();
        #[cfg(feature = "experimental-host-interrupt")]
        let usable = usable && interrupt_registry::thread_is_usable();
        usable
    }

    /// Whether nothing is installed: true before an activation first runs and
    /// after it has finished, when nothing may be left pointing into its stack.
    pub fn is_empty(&self) -> bool {
        #[cfg(feature = "experimental-host-interrupt")]
        if !self.active_stores.is_empty() {
            return false;
        }
        self.trap_handler.is_null()
    }

    /// Forgets the state of an activation whose stack is forgotten rather
    /// than unwound, while the state is not swapped in: its calls never
    /// return. What they keep beyond the thread is given up as their return
    /// would, the interrupt registrations of the stores they entered, which
    /// would otherwise keep those stores interrupted, and aimed at this
    /// thread, for good. The thread's own state is not touched: it may be
    /// gone (see [`Self::thread_is_usable`]).
    pub fn abandon(&mut self) {
        self.trap_handler = ptr::null_mut();
        #[cfg(feature = "experimental-host-interrupt")]
        self.active_stores.abandon();
    }
}

/// Read-only information that is used by signal handlers to handle and recover
/// from traps: that of the Wasm entry `ThreadState::trap_handler` points to.
///
/// Each entry coroutine keeps one (see `EntryCoroutine`), made with the
/// coroutine: the stack it covers is the coroutine's, so a call only sets the
/// custom handler before it points the thread state at it.
struct TrapHandlerContext {
    /// Information about the entry coroutine. This is used to reset execution
    /// to the root of the coroutine when a trap is handled.
    coro_trap_handler: CoroutineTrapHandler<UnwindReason>,
    /// The embedder's handler of the running call, tried first.
    custom_trap: Option<*const TrapHandlerFn<'static>>,
}

impl TrapHandlerContext {
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
            let (ptr, running_wasm) = THREAD_STATE.with(|state| {
                (
                    state.trap_handler.load(Ordering::Relaxed),
                    state.wasm_stack.get().is_some(),
                )
            });
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

            // Host code runs with the Wasm stack taken, and may run on that
            // stack (see `host_call_in_place`): a fault in it is the host's,
            // and resetting the coroutine would skip its destructors.
            if !running_wasm {
                return false;
            }

            ctx.handle_wasm_trap(pc, sp, maybe_fault_address, trap_code, &mut update_regs)
        }
    }

    /// Handles a trap if it happened on the entry coroutine's stack.
    unsafe fn handle_wasm_trap(
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
            let regs = self.coro_trap_handler.setup_trap_handler(move || unwind);
            update_regs(regs);
            true
        }
    }
}

unsafe fn unwind_with(reason: UnwindReason) -> ! {
    unsafe {
        let wasm_stack = THREAD_STATE
            .with(|state| state.wasm_stack.replace(None))
            .expect("not running on Wasm stack");

        // `on_wasm_stack` moves the reason off this frame, then forcibly
        // resets the coroutine stack.
        let mut reason = ManuallyDrop::new(reason);
        wasm_stack
            .as_ref()
            .yielder
            .as_ref()
            .suspend(&mut *reason as *mut UnwindReason);

        unreachable!();
    }
}

/// Runs the given function on a separate stack so that its stack usage can be
/// bounded. Stack overflows and other traps can be caught and execution
/// returned to the root of the stack.
///
/// The function runs as one resume of an entry coroutine of the thread (see
/// `ThreadState::entries`).
//
// Always inlined, so that the closure is reached where its caller wrote it
// (see `EntryJob`).
#[inline(always)]
fn on_wasm_stack<F: FnOnce() -> T + 'static, T: 'static>(
    state: &ThreadState,
    stack_size: usize,
    trap_handler: Option<*const TrapHandlerFn<'static>>,
    f: F,
) -> Result<T, UnwindReason> {
    unsafe fn run<F: FnOnce() -> T, T>(f: *const u8, result: *mut u8) {
        unsafe { result.cast::<T>().write(ptr::read(f.cast::<F>())()) }
    }
    // `run` moves the closure out. It is never dropped here: if the call
    // traps, it is abandoned with the rest of the Wasm stack.
    let f = ManuallyDrop::new(f);
    let mut result = MaybeUninit::<T>::uninit();
    // `entry_body` reads the call right after the resume, before any nested
    // call can set another one.
    state.job.set(EntryJob {
        run: run::<F, T>,
        f: ptr::from_ref(&*f).cast(),
        result: result.as_mut_ptr().cast(),
    });

    // Use the first entry coroutine of the thread that no running call uses,
    // in place, or make one for this call if they are all in use.
    let busy = state.entry_busy.get();
    let index = busy.trailing_ones() as usize;
    let outcome = if index < ENTRY_SLOTS && slot_ready(state, index, stack_size) {
        state.entry_busy.set(busy | (1 << index));
        let _release = SlotRelease { state, index };
        // SAFETY: the slot is this call's until `_release` releases it, and
        // `slot_ready` made sure it holds a coroutine.
        let entry = unsafe { (*state.entries[index].get()).as_mut().unwrap_unchecked() };
        unsafe { resume_entry(state, entry, trap_handler) }
    } else {
        unsafe { run_on_new_entry(state, stack_size, trap_handler) }
    };
    // SAFETY: `run` wrote the result if the call returned.
    outcome.map(|()| unsafe { result.assume_init() })
}

/// Runs the call set in `state.job` as one resume of `entry`, with the thread
/// state pointing at the coroutine's trap handler context.
///
/// # Safety
///
/// The call must be set, and nothing else may use `entry` until this returns.
#[inline(always)]
unsafe fn resume_entry(
    state: &ThreadState,
    entry: &mut EntryCoroutine,
    trap_handler: Option<*const TrapHandlerFn<'static>>,
) -> Result<(), UnwindReason> {
    entry.trap_ctx.custom_trap = trap_handler;
    compiler_fence(Ordering::Release);
    let prev = state.trap_handler.load(Ordering::Relaxed);
    state
        .trap_handler
        .store(ptr::from_mut(&mut entry.trap_ctx), Ordering::Relaxed);
    // However the call ends, even if the coroutine panics: the thread is back
    // in the host code that called, under the previous context. Host code
    // runs with the Wasm stack taken, wherever it runs; a host function that
    // called back into Wasm from a Wasm stack restores that stack itself.
    defer! {
        state.wasm_stack.set(None);
        state.trap_handler.store(prev, Ordering::Relaxed);
        compiler_fence(Ordering::Acquire);
    }
    match entry.coro.resume(()) {
        // The call returned, and the coroutine waits for the next one.
        CoroutineResult::Yield(reason) if reason.is_null() => Ok(()),
        // This came from unwind_with, which requires that there be only Wasm
        // code on the stack: take the reason off it, then discard it.
        CoroutineResult::Yield(reason) => unsafe {
            let reason = ptr::read(reason);
            entry.coro.force_reset();
            Err(reason)
        },
        // A trap handler made the coroutine return.
        CoroutineResult::Return(reason) => Err(reason),
    }
}

/// Whether entry slot `index`, which no running call uses, holds a coroutine
/// whose stack is at least `stack_size`, after filling it if it did not.
#[inline(always)]
fn slot_ready(state: &ThreadState, index: usize, stack_size: usize) -> bool {
    // SAFETY: no running call uses the slot, so nothing refers into it.
    let slot = unsafe { &mut *state.entries[index].get() };
    matches!(slot, Some(entry) if entry.stack_size >= stack_size)
        || fill_slot(state, slot, stack_size)
}

/// Releases the entry slot of a call when the call ends, and retires the
/// slot's coroutine if it finished (after a trap or a panic).
struct SlotRelease<'a> {
    state: &'a ThreadState,
    index: usize,
}

impl Drop for SlotRelease<'_> {
    #[inline(always)]
    fn drop(&mut self) {
        let state = self.state;
        state
            .entry_busy
            .set(state.entry_busy.get() & !(1 << self.index));
        // SAFETY: the slot was the call's, which no longer runs.
        let slot = unsafe { &mut *state.entries[self.index].get() };
        if slot.as_ref().is_some_and(|entry| entry.coro.done()) {
            retire_slot(state, slot);
        }
    }
}

/// The yielder of an entry coroutine.
///
/// The values that cross a switch are at most a word, which corosensei passes
/// in a register; larger ones go through memory and are read right after
/// being written. So the call comes through `ThreadState::job`, a suspension
/// yields null when the call returned or a pointer to the reason given to
/// `unwind_with`, and only the rare return from a trap carries the reason
/// itself.
type EntryYielder = Yielder<(), *mut UnwindReason>;

/// One call for an entry coroutine: `run(f, result)` on its stack, which calls
/// the closure at `f` and writes what it returns to `result`.
///
/// The closure and the place for its result stay on the frame of
/// `on_wasm_stack`, reached through type-erased pointers. The closure is not
/// copied: its caller has just written it field by field, and a bulk copy
/// would read bytes from several of those stores at once, which waits for the
/// stores to reach the cache instead of taking their values.
#[derive(Clone, Copy)]
struct EntryJob {
    run: unsafe fn(*const u8, *mut u8),
    f: *const u8,
    result: *mut u8,
}

impl EntryJob {
    /// What `ThreadState::job` holds before the first call.
    const NONE: Self = {
        unsafe fn none(_: *const u8, _: *mut u8) {
            unreachable!("an entry coroutine resumed without a call");
        }
        Self {
            run: none,
            f: ptr::null(),
            result: ptr::null_mut(),
        }
    };
}

/// A coroutine that runs calls on a Wasm stack, one per resume (see
/// `entry_body`). It suspends with null after each call and with the reason
/// from `unwind_with`; a trap handler makes it return. Between two calls
/// nothing on its stack needs dropping, so it can be reset at any suspension.
struct EntryCoroutine {
    coro: Coroutine<(), *mut UnwindReason, UnwindReason, DefaultStack>,
    /// The size of its stack, compared with the size a call asks for.
    stack_size: usize,
    /// What the signal handler uses while a call runs on the coroutine.
    trap_ctx: TrapHandlerContext,
}

/// The Wasm stack of an entry coroutine, which `ThreadState::wasm_stack`
/// points to while Wasm runs on it: what it takes to leave the stack (its
/// yielder) and to measure what is left of it. It lives on that very stack,
/// in the frame of `entry_body`, for as long as the coroutine does, so taking
/// and restoring the pointer keeps the two together.
struct WasmStack {
    yielder: NonNull<EntryYielder>,
    /// The lowest address code may use on the stack, above its guard page.
    limit: usize,
}

/// The body of every entry coroutine: runs the call set in `state.job`, then
/// suspends until the next one. `limit` is that of `WasmStack`.
fn entry_body(state: *const ThreadState, limit: usize, yielder: &EntryYielder) -> UnwindReason {
    let wasm_stack = WasmStack {
        yielder: NonNull::from(yielder),
        limit,
    };
    loop {
        // SAFETY: the coroutine runs on the thread that made it, whose state
        // lives at least as long as the coroutine (it is retired at thread
        // exit), and `on_wasm_stack` set the call right before resuming it.
        unsafe {
            let state = &*state;
            state.wasm_stack.set(Some(NonNull::from(&wasm_stack)));
            let job = state.job.get();
            (job.run)(job.f, job.result);
        }
        yielder.suspend(ptr::null_mut());
    }
}

/// Fills an empty entry slot, or replaces a coroutine whose stack is smaller
/// than a call asks for (discarded, as `acquire_stack` does with stacks).
///
/// A coroutine may only be kept once something will return its stack when
/// the thread exits: if the thread is past the point where `ThreadExit` can
/// be registered, the slot stays as it is and this returns false.
#[cold]
#[inline(never)]
fn fill_slot(state: &ThreadState, slot: &mut Option<EntryCoroutine>, stack_size: usize) -> bool {
    if state.flags.get() & EXIT_HOOK_REGISTERED == 0 && !state.register_exit_hook() {
        return false;
    }
    if let Some(undersized) = slot.take() {
        drop(retire_entry(undersized));
    }
    *slot = Some(new_entry(state, stack_size));
    true
}

/// Gives the stack of a slot's coroutine that finished back to the stack
/// cache, emptying the slot.
#[cold]
#[inline(never)]
fn retire_slot(state: &ThreadState, slot: &mut Option<EntryCoroutine>) {
    if let Some(entry) = slot.take() {
        release_stack(state, retire_entry(entry));
    }
}

/// Runs the call set in `state.job` on a coroutine of its own, when every
/// entry slot is in use (calls nested deeper, or activations of `call_async`
/// suspended in Wasm) or the thread is exiting. Its stack goes back to the
/// stack cache afterwards.
///
/// # Safety
///
/// The call must be set.
#[cold]
#[inline(never)]
unsafe fn run_on_new_entry(
    state: &ThreadState,
    stack_size: usize,
    trap_handler: Option<*const TrapHandlerFn<'static>>,
) -> Result<(), UnwindReason> {
    let mut entry = scopeguard::guard(new_entry(state, stack_size), |entry| {
        release_stack(state, retire_entry(entry))
    });
    unsafe { resume_entry(state, &mut entry, trap_handler) }
}

/// Makes an entry coroutine on a stack from `acquire_stack`.
#[cold]
#[inline(never)]
fn new_entry(state: &ThreadState, stack_size: usize) -> EntryCoroutine {
    let stack = acquire_stack(state, stack_size);
    let stack_size = stack.size();
    // The limit includes the guard page (`Stack::limit`); host code measures
    // what is left above it.
    let limit = stack.limit().get() + region::page::size();
    let state = state as *const ThreadState;
    let coro = Coroutine::with_stack(stack, move |yielder, ()| entry_body(state, limit, yielder));
    let trap_ctx = TrapHandlerContext {
        coro_trap_handler: coro.trap_handler(),
        custom_trap: None,
    };
    EntryCoroutine {
        coro,
        stack_size,
        trap_ctx,
    }
}

/// Turns an entry coroutine that is not running back into its stack.
fn retire_entry(mut entry: EntryCoroutine) -> DefaultStack {
    // SAFETY: a coroutine that is not running has finished, has not started,
    // or is suspended in `entry_body` between two calls; nothing on its stack
    // needs dropping in any of these cases.
    unsafe { entry.coro.force_reset() };
    entry.coro.into_stack()
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
    // One thread-local lookup for the take and the restore below: the host
    // call returns to this thread before the restore runs.
    with_thread_state(|state| {
        // If we are already on the host stack, execute the function directly.
        // This happens if a host function is called directly from the API.
        let Some(wasm_stack) = state.wasm_stack.get() else {
            return f();
        };
        let yielder = unsafe { wasm_stack.as_ref().yielder.as_ref() };

        // on_parent_stack requires the closure to be Send so that the Yielder
        // cannot be called from the parent stack. This is not a problem for us
        // since we don't expose the Yielder.
        struct SendWrapper<T>(T);
        unsafe impl<T> Send for SendWrapper<T> {}
        let wrapped = SendWrapper((f, ptr::from_ref(state), wasm_stack));
        yielder.on_parent_stack(move || {
            let wrapped = wrapped;
            let (f, state, wasm_stack) = wrapped.0;
            // SAFETY: this thread's state; the closure runs on this thread.
            let state = unsafe { &*state };
            // Take the Wasm stack for as long as `f` runs, once on the host
            // stack, and give it back before switching back to the Wasm stack,
            // whether `f` returns or unwinds. The switch itself, on either
            // side, runs on the Wasm stack as the guest's: overflowing it there
            // is the guest's stack overflow, which the signal handler turns
            // into a trap, as it does in the guest's own frames.
            state.wasm_stack.set(None);
            defer! {
                state.wasm_stack.set(Some(wasm_stack));
            }
            f()
        })
    })
}

/// A host function called from Wasm that runs where it was called, without
/// switching stacks (see [`host_call_in_place`]). Gives the thread its Wasm
/// stack back when dropped, which must happen on the thread and before the
/// host function returns to Wasm.
#[must_use = "the host call ends when this is dropped"]
pub struct HostCallInPlace {
    state: NonNull<ThreadState>,
    /// The Wasm stack the host function was called on, if any.
    wasm_stack: Option<NonNull<WasmStack>>,
}

impl Drop for HostCallInPlace {
    #[inline(always)]
    fn drop(&mut self) {
        // SAFETY: this thread's state (the guard is not `Send`), which
        // outlives the host call.
        unsafe { self.state.as_ref() }
            .wasm_stack
            .set(self.wasm_stack);
    }
}

/// Starts a call of a synchronous host function from Wasm where it stands, on
/// the Wasm stack, if at least [`HOST_CALL_RESERVE`] is left on that stack;
/// returns `None` otherwise, and the host function must then run through
/// [`on_host_stack`].
///
/// Like `on_host_stack`, it takes the Wasm stack for as long as the returned
/// guard lives: the host code runs as host code, which unwinding from inside
/// it (`raise_user_trap` and the like) and the signal handler rely on. Called
/// outside of Wasm, it returns a guard that does nothing, as `on_host_stack`
/// then runs its function in place.
///
/// A host function that may suspend the coroutine it was called from, as an
/// asynchronous one does, must not use this.
#[inline(always)]
pub fn host_call_in_place() -> Option<HostCallInPlace> {
    host_call_in_place_on(CurrentThread::get())
}

/// [`host_call_in_place`] on `thread`, which must be the calling thread's:
/// for a caller that has it at hand, such as from a [`StoreSlot`] the call
/// into the store filled, and so saves looking the thread up.
///
/// [`StoreSlot`]: crate::StoreSlot
#[inline(always)]
pub fn host_call_in_place_on(thread: CurrentThread) -> Option<HostCallInPlace> {
    debug_assert!(
        thread.state == CurrentThread::get().state,
        "a host call on another thread's state"
    );
    let state = thread.state;
    let state_ref = thread.state();
    let wasm_stack = state_ref.wasm_stack.get();
    if let Some(stack) = wasm_stack {
        // SAFETY: it lives on the stack of the coroutine that is running.
        let limit = unsafe { stack.as_ref() }.limit;
        // Below the limit is none: a frame can reach below it, into the guard
        // page, before it touches the page. Then the host function must not
        // run as host code there, where the signal handler leaves its fault
        // alone and the process dies; switching stacks faults as the guest.
        let left = stack_pointer().saturating_sub(limit);
        if left < HOST_CALL_RESERVE {
            return None;
        }
        state_ref.wasm_stack.set(None);
    }
    Some(HostCallInPlace { state, wasm_stack })
}

/// The current stack pointer, or an address in the current frame where it
/// cannot be read directly.
#[inline(always)]
fn stack_pointer() -> usize {
    let sp: usize;
    cfg_select! {
        target_arch = "aarch64" => {
            unsafe { std::arch::asm!("mov {}, sp", out(reg) sp, options(nomem, nostack, preserves_flags)) };
        }
        target_arch = "x86_64" => {
            unsafe { std::arch::asm!("mov {}, rsp", out(reg) sp, options(nomem, nostack, preserves_flags)) };
        }
        target_arch = "riscv64" => {
            unsafe { std::arch::asm!("mv {}, sp", out(reg) sp, options(nomem, nostack, preserves_flags)) };
        }
        target_arch = "loongarch64" => {
            unsafe { std::arch::asm!("move {}, $sp", out(reg) sp, options(nomem, nostack, preserves_flags)) };
        }
        _ => {
            let probe = 0u8;
            sp = std::hint::black_box(ptr::from_ref(&probe)) as usize;
        }
    }
    sp
}

/// Called on every entry into WebAssembly.
#[cfg(windows)]
fn lazy_per_thread_init() -> Result<(), Trap> {
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
///
/// Called by [`ThreadState::per_thread_init`] until it succeeds on the thread;
/// it then sets `SIGNAL_STACK_READY`, and the signal stack's destructor clears
/// it again, so an entry after that point runs this again as it always did.
#[cfg(unix)]
#[cold]
#[inline(never)]
fn lazy_per_thread_init(state: &ThreadState) -> Result<(), Trap> {
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
            state.set_flag(SIGNAL_STACK_READY);
            Ok(())
        }
    });

    impl Drop for Tls {
        fn drop(&mut self) {
            // The signal stack is gone from here on: send the next entry on
            // this thread back through `lazy_per_thread_init`.
            let _ = THREAD_STATE.try_with(|state| state.clear_flag(SIGNAL_STACK_READY));
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
        clear_tls_stack();

        // Push an undersized stack into the pool.
        let small_size = ByteSize::kib(500).as_u64() as usize;
        let small_stack = DefaultStack::new(small_size).unwrap();
        STACK_POOL.push(small_stack);

        // Request a larger stack via on_wasm_stack.
        let big_size = ByteSize::mib(1).as_u64() as usize;
        let result = on_wasm_stack(big_size, None, || 42);

        assert_eq!(result.expect("on_wasm_stack should succeed"), 42);
        // The undersized stack was discarded; the call ran on a coroutine on
        // a correctly-sized stack, which the thread keeps for the next call.
        // It ends up in the global pool only on thread exit or eviction.
        assert!(STACK_POOL.is_empty(), "the undersized stack is discarded");
        let kept = cached_entries()[0].expect("the call's coroutine stays cached");
        assert!(
            kept >= big_size,
            "the kept stack must be at least as large as the requested size"
        );

        // Ensure no residual TLS state leaks into other tests sharing the
        // runner thread.
        clear_tls_stack();
    }

    /// After a wasm call, the thread keeps the entry coroutine it ran on,
    /// suspended, and the next call on the same thread runs on the same one
    /// without touching the global SegQueue.
    #[test]
    fn the_next_call_reuses_the_entry_coroutine() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let size = get_stack_size();

        // First call: nothing cached → a coroutine on a fresh stack, kept in
        // the first slot with its stack.
        let first = on_wasm_stack(size, None, running_entry).unwrap();
        assert!(
            STACK_POOL.is_empty(),
            "pool should still be empty after the first call"
        );
        assert!(cached_entries()[0].is_some(), "the coroutine is kept");
        assert!(!has_cached_stack(), "its stack is in the coroutine");

        // Second call: same coroutine; pool stays empty.
        let second = on_wasm_stack(size, None, running_entry).unwrap();
        assert_eq!(first, second, "the second call runs on the same coroutine");
        assert!(
            STACK_POOL.is_empty(),
            "second call must not push to the global pool"
        );

        clear_tls_stack();
    }

    /// A host function calling back into Wasm runs on the second entry
    /// coroutine. Both are kept, so repeated calls reuse both.
    #[test]
    fn a_call_back_into_wasm_reuses_the_second_entry_coroutine() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let size = get_stack_size();
        let call = move || {
            on_wasm_stack(size, None, move || {
                let outer = running_entry();
                let inner = on_host_stack(|| on_wasm_stack(size, None, running_entry).unwrap());
                (outer, inner)
            })
            .unwrap()
        };
        let (outer, inner) = call();
        assert_ne!(outer, inner, "the call back runs on another coroutine");
        assert_eq!(call(), (outer, inner), "both coroutines are reused");
        let [first, second] = cached_entries();
        assert!(first.is_some() && second.is_some(), "both are kept");
        assert!(STACK_POOL.is_empty());

        clear_tls_stack();
    }

    /// Calls nested deeper than there are slots each make a coroutine of their
    /// own, whose stack goes back to the stack cache when they return.
    #[test]
    fn calls_nested_beyond_the_slots_make_their_own_coroutine() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let size = get_stack_size();
        let busy_in_third = on_wasm_stack(size, None, move || {
            on_host_stack(|| {
                on_wasm_stack(size, None, move || {
                    on_host_stack(|| {
                        on_wasm_stack(size, None, || THREAD_STATE.with(|s| s.entry_busy.get()))
                            .unwrap()
                    })
                })
                .unwrap()
            })
        })
        .unwrap();
        assert_eq!(busy_in_third, 0b11, "both slots were in use");
        let [first, second] = cached_entries();
        assert!(first.is_some() && second.is_some());
        assert!(has_cached_stack(), "the third call's stack is cached");

        clear_tls_stack();
        drain_stack_pool();
    }

    /// A slot held by a call that has not returned (an activation of
    /// `call_async` suspended in Wasm, say; marked busy by hand here) is
    /// skipped, whatever its index: calls take the first free one.
    #[test]
    fn calls_skip_a_slot_in_use() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let size = get_stack_size();
        THREAD_STATE.with(|s| s.entry_busy.set(0b01));
        let in_second = on_wasm_stack(size, None, running_entry).unwrap();
        THREAD_STATE.with(|s| {
            assert_eq!(s.entry_busy.get(), 0b01, "the held slot stays held");
            s.entry_busy.set(0);
        });
        let [first, second] = cached_entries();
        assert!(first.is_none() && second.is_some(), "the call used slot 1");

        let in_first = on_wasm_stack(size, None, running_entry).unwrap();
        assert_ne!(in_first, in_second, "slot 0 is free again and gets its own");

        clear_tls_stack();
        drain_stack_pool();
    }

    /// A trap discards the entry coroutine it ran on. Its stack goes back to
    /// the stack cache, and the next call makes a new coroutine on it.
    #[test]
    fn a_trap_retires_the_entry_coroutine() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let size = get_stack_size();
        let r = on_wasm_stack(size, None, || -> () {
            unsafe { raise_lib_trap(Trap::lib(TrapCode::UnreachableCodeReached)) }
        });
        assert!(matches!(r, Err(UnwindReason::LibTrap(_))));
        assert_eq!(cached_entries(), [None, None]);
        assert!(has_cached_stack(), "the stack is back in the stack cache");

        assert_eq!(on_wasm_stack(size, None, || 5).unwrap(), 5);
        assert!(cached_entries()[0].is_some());
        assert!(!has_cached_stack(), "the new coroutine took the stack");

        clear_tls_stack();
        drain_stack_pool();
    }

    /// A panic in the call reaches the caller, and the coroutine it ended is
    /// not kept; the thread's state is as before the call.
    #[test]
    fn a_panic_in_the_call_retires_the_entry_coroutine() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let size = get_stack_size();
        let r = std::panic::catch_unwind(|| {
            on_wasm_stack(size, None, || -> () { panic!("panic in the call") })
        });
        assert!(r.is_err());
        THREAD_STATE.with(|s| {
            assert_eq!(s.entry_busy.get(), 0);
            assert!(s.wasm_stack.get().is_none());
            assert!(s.trap_handler.load(Ordering::Relaxed).is_null());
        });
        assert_eq!(cached_entries(), [None, None]);
        assert_eq!(on_wasm_stack(size, None, || 6).unwrap(), 6);

        clear_tls_stack();
        drain_stack_pool();
    }

    /// On thread exit, `ThreadExit` returns the stack of the entry coroutine
    /// cached in `ThreadState` to the global pool so memory cycles correctly
    /// across thread lifetimes.
    #[test]
    fn tls_stack_returns_to_pool_on_thread_exit() {
        // Keep other tests from draining the pool or changing the stack size
        // until the worker exits and we inspect its returned stack. The worker
        // calls on_wasm_stack without acquiring this test-only mutex.
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let size = get_stack_size();
        let handle = std::thread::spawn(move || {
            assert!(on_wasm_stack(size, None, || ()).is_ok());
        });
        handle.join().unwrap();

        // The spawned thread's `ThreadExit` ran before the join returned; the
        // stack must have made it back to the global pool.
        let returned = STACK_POOL
            .pop()
            .expect("thread exit should return TLS-cached stack to the global pool");
        assert!(returned.size() >= size);
    }

    /// A call made from a thread-local destructor that runs after
    /// `ThreadExit`'s cannot cache its stack, which nothing would return any
    /// more: the stack goes to the global pool, and nothing stays cached on
    /// the exiting thread. (Destructors run in reverse order of registration
    /// on these targets, so one registered before `ThreadExit` runs after it.)
    #[cfg(any(target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn a_call_after_thread_exit_returns_its_stack_to_the_pool() {
        use std::cell::RefCell;
        use std::sync::mpsc;
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        /// What the late call saw: its result, whether `ThreadExit` had run
        /// before it, the flags after it, and whether a stack stayed cached.
        type Seen = (i32, bool, u8, bool);
        struct Late(mpsc::Sender<Seen>);
        impl Drop for Late {
            fn drop(&mut self) {
                let exit_ran = THREAD_STATE.with(|s| s.flags.get() & EXIT_HOOK_REGISTERED == 0);
                let result = on_wasm_stack(get_stack_size(), None, || 7).unwrap();
                let flags = THREAD_STATE.with(|s| s.flags.get());
                let cached = has_cached_stack() || cached_entries() != [None, None];
                self.0.send((result, exit_ran, flags, cached)).unwrap();
            }
        }
        thread_local! {
            static LATE: RefCell<Option<Late>> = const { RefCell::new(None) };
        }
        let (tx, rx) = mpsc::channel();
        let size = get_stack_size();
        std::thread::spawn(move || {
            // Registered before the first call, which registers `ThreadExit`
            // when it fills its entry slot.
            LATE.with(|late| *late.borrow_mut() = Some(Late(tx)));
            assert!(on_wasm_stack(size, None, || ()).is_ok());
        })
        .join()
        .unwrap();

        let (result, exit_ran, flags, cached) = rx.recv().unwrap();
        assert_eq!(result, 7);
        assert!(exit_ran, "ThreadExit ran before the late call");
        assert_eq!(flags & EXIT_HOOK_REGISTERED, 0);
        assert!(!cached, "nothing may stay cached after ThreadExit ran");
        // The stack `ThreadExit` returned, which the late call took and gave
        // back, is in the pool.
        let mut pooled = 0;
        while STACK_POOL.pop().is_some() {
            pooled += 1;
        }
        assert!(pooled >= 1, "the late call's stack is in the pool");
    }

    /// Many threads entering, trapping, re-entering through the host stack
    /// and exiting: every stack comes back, and the pool stays bounded by the
    /// number of threads alive at once.
    #[test]
    fn stacks_of_exiting_threads_stay_bounded() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();
        let size = get_stack_size();
        const THREADS: usize = 16;
        for _wave in 0..30 {
            let handles: Vec<_> = (0..THREADS)
                .map(|i| {
                    std::thread::spawn(move || {
                        for j in 0..50 {
                            let traps = (i + j) % 2 == 0;
                            let result: Result<(), UnwindReason> =
                                on_wasm_stack(size, None, move || unsafe {
                                    if traps {
                                        raise_lib_trap(Trap::lib(TrapCode::UnreachableCodeReached));
                                    }
                                    on_host_stack(|| {
                                        assert_eq!(on_wasm_stack(size, None, || 3).unwrap(), 3);
                                    });
                                });
                            assert_eq!(result.is_err(), traps);
                        }
                    })
                })
                .collect();
            for handle in handles {
                handle.join().unwrap();
            }
        }
        let mut pooled = 0;
        while STACK_POOL.pop().is_some() {
            pooled += 1;
        }
        // At most two stacks (outer and nested call) per thread alive at once.
        assert!(pooled <= 2 * THREADS, "pool grew to {pooled} stacks");
    }

    // -----------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------

    // The functions under test take the thread's state; these wrappers look
    // it up for each call the way `catch_traps` does, and shadow the
    // originals in this module.
    fn on_wasm_stack<F: FnOnce() -> T + 'static, T: 'static>(
        stack_size: usize,
        trap_handler: Option<*const TrapHandlerFn<'static>>,
        f: F,
    ) -> Result<T, UnwindReason> {
        THREAD_STATE.with(|state| super::on_wasm_stack(state, stack_size, trap_handler, f))
    }

    fn acquire_stack(min_size: usize) -> DefaultStack {
        THREAD_STATE.with(|state| super::acquire_stack(state, min_size))
    }

    fn release_stack(stack: DefaultStack) {
        THREAD_STATE.with(|state| super::release_stack(state, stack))
    }

    /// The current thread's cached stack slot.
    fn cached_stack<R>(f: impl FnOnce(&Cell<Option<DefaultStack>>) -> R) -> R {
        THREAD_STATE.with(|state| f(&state.stack))
    }

    /// Whether the current thread caches a stack (outside its entry
    /// coroutines).
    fn has_cached_stack() -> bool {
        cached_stack(|cache| {
            let stack = cache.take();
            let cached = stack.is_some();
            cache.set(stack);
            cached
        })
    }

    /// The stack sizes of the current thread's cached entry coroutines, by
    /// slot. Only between calls.
    fn cached_entries() -> [Option<usize>; ENTRY_SLOTS] {
        THREAD_STATE.with(|state| {
            assert_eq!(state.entry_busy.get(), 0, "no call is running");
            // SAFETY: no call is running, so nothing refers into the slots.
            std::array::from_fn(|i| unsafe {
                (*state.entries[i].get())
                    .as_ref()
                    .map(|entry| entry.stack_size)
            })
        })
    }

    /// Identifies the entry coroutine running the current call: the address
    /// of its `WasmStack`, which lives on its stack.
    fn running_entry() -> usize {
        THREAD_STATE.with(|state| {
            state
                .wasm_stack
                .get()
                .expect("running on a Wasm stack")
                .as_ptr() as usize
        })
    }

    /// Clears the current thread's cached stack and entry coroutines so tests
    /// don't see state from previous tests (they share the same thread under
    /// cargo test's per-test serialization via `GLOBAL_STATE`).
    fn clear_tls_stack() {
        THREAD_STATE.with(|state| state.take_idle_entries().for_each(drop));
        cached_stack(|cache| cache.set(None));
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
        assert!(
            stack.size() >= size,
            "freshly allocated stack must satisfy min_size"
        );

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
        cached_stack(|cache| cache.set(Some(tls_stack)));

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
        assert_eq!(
            stack_id(&got),
            pool_id,
            "acquire must consume from pool when TLS is empty"
        );
        assert!(
            STACK_POOL.is_empty(),
            "pool stack must be removed when used"
        );

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
        cached_stack(|cache| cache.set(Some(undersized)));

        let big_size = ByteSize::mib(2).as_u64() as usize;
        let got = acquire_stack(big_size);

        // The acquired stack must be at least the requested size. We do NOT
        // compare base addresses: the OS can reuse a freshly munmap'd
        // virtual address for the next mmap, so pointer identity is not a
        // reliable "is this a different stack" check across a drop+alloc.
        // The meaningful semantic is that the undersized stack was taken
        // out of rotation (TLS empty, not silently pushed to the pool) and
        // the returned stack is sized correctly.
        assert!(
            got.size() >= big_size,
            "acquired stack must satisfy big_size"
        );
        let tls_empty = cached_stack(|cache| {
            let s = cache.take();
            let empty = s.is_none();
            cache.set(s);
            empty
        });
        assert!(
            tls_empty,
            "undersized TLS stack must have been taken and discarded"
        );
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
        assert!(
            got.size() >= big_size,
            "acquired stack must satisfy big_size"
        );
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

        let in_tls = cached_stack(|cache| cache.take())
            .expect("release into empty TLS should leave the stack in TLS");
        assert_eq!(stack_id(&in_tls), id);
        assert!(
            STACK_POOL.is_empty(),
            "pool must not be touched when TLS is empty"
        );

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
        cached_stack(|cache| cache.set(Some(older)));

        let newer = DefaultStack::new(size).unwrap();
        let newer_id = stack_id(&newer);
        release_stack(newer);

        let in_tls =
            cached_stack(|cache| cache.take()).expect("TLS should hold the newly-released stack");
        assert_eq!(
            stack_id(&in_tls),
            newer_id,
            "newer stack must displace into TLS"
        );

        let displaced = STACK_POOL
            .pop()
            .expect("older stack should have been pushed to global pool");
        assert_eq!(
            stack_id(&displaced),
            older_id,
            "displaced stack must be the older one"
        );

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
        cached_stack(|cache| cache.set(Some(stack)));

        drain_stack_pool();

        let tls_empty = cached_stack(|cache| cache.take().is_none());
        assert!(
            tls_empty,
            "drain_stack_pool must also clear current thread's TLS slot"
        );
        assert!(STACK_POOL.is_empty());
    }

    /// `drain_stack_pool` also drops the calling thread's cached entry
    /// coroutines, whose stacks may be undersized as well.
    #[test]
    fn drain_stack_pool_retires_the_calling_threads_entry_coroutines() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        drain_stack_pool();
        clear_tls_stack();

        assert!(on_wasm_stack(get_stack_size(), None, || ()).is_ok());
        assert!(cached_entries()[0].is_some());

        drain_stack_pool();

        assert_eq!(cached_entries(), [None, None]);
        assert!(!has_cached_stack());
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

        let r: Result<(), UnwindReason> = on_wasm_stack(get_stack_size(), None, || unsafe {
            raise_user_trap(Box::new(io::Error::other("user trap from test")));
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

        let r: Result<(), UnwindReason> = on_wasm_stack(get_stack_size(), None, || unsafe {
            raise_lib_trap(Trap::lib(TrapCode::IntegerDivisionByZero));
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

        let r: Result<(), UnwindReason> = on_wasm_stack(get_stack_size(), None, || unsafe {
            resume_panic(Box::new("panic payload from test"));
        });
        assert!(
            r.is_err(),
            "resume_panic must surface as Err to on_wasm_stack"
        );

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

        let trapped: Result<(), UnwindReason> = on_wasm_stack(get_stack_size(), None, || unsafe {
            raise_user_trap(Box::new(io::Error::other("first call traps")));
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
    // Host calls in place
    // -----------------------------------------------------------------

    fn current_wasm_stack() -> Option<NonNull<WasmStack>> {
        THREAD_STATE.with(|state| state.wasm_stack.get())
    }

    #[test]
    fn a_host_call_outside_wasm_runs_in_place() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let in_place = host_call_in_place().expect("there is no stack to switch from");
        assert!(current_wasm_stack().is_none());
        drop(in_place);
        assert!(current_wasm_stack().is_none());
    }

    /// With the reserve left, a host call stays on the Wasm stack, runs as host
    /// code there, and gives the stack back when it ends.
    #[test]
    fn a_host_call_with_room_runs_on_the_wasm_stack() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let r = on_wasm_stack(get_stack_size(), None, || {
            let outer = current_wasm_stack().expect("running on a Wasm stack");
            let in_place = host_call_in_place().expect("a fresh Wasm stack has room");
            assert!(
                current_wasm_stack().is_none(),
                "the host call runs as host code"
            );
            // Still on the Wasm stack: above its limit and below its
            // `WasmStack`, which `entry_body` keeps near its top.
            let sp = stack_pointer();
            let limit = unsafe { outer.as_ref() }.limit;
            assert!(sp > limit && sp < outer.as_ptr() as usize);
            drop(in_place);
            assert_eq!(current_wasm_stack(), Some(outer));
        });
        assert!(r.is_ok());

        clear_tls_stack();
        drain_stack_pool();
    }

    /// Without the reserve left, the host call has to switch stacks.
    #[test]
    fn a_host_call_without_room_switches_stacks() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let r = on_wasm_stack(get_stack_size(), None, || {
            let outer = current_wasm_stack().expect("running on a Wasm stack");
            // The stack as a frame with less than the reserve left sees it.
            let short = WasmStack {
                yielder: unsafe { outer.as_ref() }.yielder,
                limit: stack_pointer() - HOST_CALL_RESERVE / 2,
            };
            let short = NonNull::from(&short);
            THREAD_STATE.with(|state| state.wasm_stack.set(Some(short)));
            assert!(host_call_in_place().is_none());
            // Nothing was taken.
            assert_eq!(current_wasm_stack(), Some(short));
            let switched = on_host_stack(|| current_wasm_stack().is_none());
            THREAD_STATE.with(|state| state.wasm_stack.set(Some(outer)));
            switched
        });
        assert_eq!(r.ok(), Some(true));

        clear_tls_stack();
        drain_stack_pool();
    }

    /// Outer Wasm -> host call in place -> inner Wasm: the inner call leaves
    /// the host code as it found it, the host call gives the outer stack
    /// back, and the outer call can still unwind from it.
    #[test]
    fn a_call_back_into_wasm_from_a_host_call_in_place() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let size = get_stack_size();
        let r: Result<(), UnwindReason> = on_wasm_stack(size, None, move || {
            let outer = current_wasm_stack().expect("running on a Wasm stack");
            let in_place = host_call_in_place().expect("a fresh Wasm stack has room");
            let inner = on_wasm_stack(size, None, move || {
                let inner = current_wasm_stack().expect("running on a Wasm stack");
                assert_ne!(inner, outer);
                5
            });
            assert_eq!(inner.ok(), Some(5));
            assert!(current_wasm_stack().is_none(), "still in host code");
            drop(in_place);
            assert_eq!(current_wasm_stack(), Some(outer));
            unsafe { raise_lib_trap(Trap::lib(TrapCode::IntegerDivisionByZero)) }
        });
        assert!(r.is_err(), "the outer call unwinds from its own stack");

        clear_tls_stack();
        drain_stack_pool();
    }

    /// A host call from a frame that reaches below the limit, into the guard
    /// page, without having touched it yet, has no room left: it switches
    /// stacks. Run in place, as host code, it would fault there, and the
    /// signal handler would leave the fault alone.
    #[test]
    fn a_host_call_from_below_the_limit_switches_stacks() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let r = on_wasm_stack(get_stack_size(), None, || {
            let outer = current_wasm_stack().expect("running on a Wasm stack");
            // The stack as such a frame sees it: its limit above the frame.
            let below = WasmStack {
                yielder: unsafe { outer.as_ref() }.yielder,
                limit: stack_pointer() + region::page::size(),
            };
            THREAD_STATE.with(|state| state.wasm_stack.set(Some(NonNull::from(&below))));
            let in_place = host_call_in_place();
            let switches = in_place.is_none();
            drop(in_place);
            THREAD_STATE.with(|state| state.wasm_stack.set(Some(outer)));
            switches
        });
        assert_eq!(r.ok(), Some(true));

        clear_tls_stack();
        drain_stack_pool();
    }

    /// A fault on the Wasm stack while host code runs there is the host's: the
    /// signal handler must not reset the coroutine under it.
    #[test]
    fn the_signal_handler_leaves_host_code_on_the_wasm_stack_alone() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let r = on_wasm_stack(get_stack_size(), None, || {
            let in_place = host_call_in_place().expect("a fresh Wasm stack has room");
            let sp = stack_pointer();
            let handled = unsafe {
                TrapHandlerContext::handle_trap(
                    0,
                    sp,
                    Some(sp),
                    Some(TrapCode::StackOverflow),
                    |_| panic!("the host code's registers were rewritten"),
                    |_| false,
                )
            };
            drop(in_place);
            handled
        });
        assert_eq!(r.ok(), Some(false));

        clear_tls_stack();
        drain_stack_pool();
    }

    // -----------------------------------------------------------------
    // Thread state
    // -----------------------------------------------------------------

    /// `ActivationState::swap` exchanges the trap handler, and leaves the
    /// thread's cached entry coroutines and its one-time setup alone: they
    /// belong to the thread, not to the activation that is switched out.
    #[test]
    fn activation_swap_exchanges_only_the_trap_handler() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        drain_stack_pool();
        clear_tls_stack();

        assert!(on_wasm_stack(get_stack_size(), None, || ()).is_ok());
        let cached = cached_entries();
        assert!(cached[0].is_some(), "the call leaves its coroutine cached");
        let flags = THREAD_STATE.with(|state| state.flags.get());
        assert_ne!(flags & EXIT_HOOK_REGISTERED, 0);

        // A suspended activation's trap handler. Never dereferenced: nothing
        // traps while it is installed.
        let outer = NonNull::<TrapHandlerContext>::dangling().as_ptr();
        let mut activation = ActivationState::new();
        activation.trap_handler = outer;
        unsafe { activation.swap() };
        THREAD_STATE.with(|state| {
            assert_eq!(state.trap_handler.load(Ordering::Relaxed), outer);
            assert_eq!(state.flags.get(), flags);
            assert!(state.wasm_stack.get().is_none());
        });
        assert_eq!(cached_entries(), cached);
        unsafe { activation.swap() };
        assert_eq!(activation.trap_handler, outer);
        THREAD_STATE.with(|state| {
            assert!(state.trap_handler.load(Ordering::Relaxed).is_null());
        });

        clear_tls_stack();
        drain_stack_pool();
    }

    /// The signal stack is set up by the first entry on a thread, and later
    /// entries skip that work.
    #[cfg(unix)]
    #[test]
    fn per_thread_init_is_remembered() {
        std::thread::spawn(|| {
            THREAD_STATE.with(|state| {
                assert_eq!(state.flags.get() & SIGNAL_STACK_READY, 0);
                state.per_thread_init().unwrap();
                assert_ne!(state.flags.get() & SIGNAL_STACK_READY, 0);
                state.per_thread_init().unwrap();
            });
        })
        .join()
        .unwrap();
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
            on_wasm_stack(get_stack_size(), None, || 42i32).expect("inner must succeed")
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
            let inner: Result<i32, UnwindReason> =
                on_wasm_stack(get_stack_size(), None, || unsafe {
                    raise_user_trap(Box::new(io::Error::other("inner trap")));
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
        // save/restore of `ThreadState::wasm_stack` in `unwind_with` /
        // `on_host_stack` against a re-entrant boundary.
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        let r = on_wasm_stack(get_stack_size(), None, || {
            on_host_stack(|| {
                on_wasm_stack(get_stack_size(), None, || 5i32).expect("nested inner must succeed")
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
        // exited). Each thread's `ThreadExit` also returns the stack cached in
        // its `ThreadState` to the pool on exit.
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
    fn growing_request_discards_a_smaller_entry_coroutine() {
        let _lock = GLOBAL_STATE.lock().unwrap();
        let _restore = RestoreStackSize(get_stack_size());
        drain_stack_pool();
        clear_tls_stack();

        // First call at a small size leaves a coroutine on a small stack.
        let small = ByteSize::mib(1).as_u64() as usize;
        set_stack_size(small);
        assert!(on_wasm_stack(small, None, || ()).is_ok());

        let cached_size = cached_entries()[0].unwrap_or(0);
        assert!(
            cached_size >= small,
            "the thread should keep a coroutine on the small-sized stack"
        );

        // Now request a larger stack. The undersized coroutine is discarded,
        // and the new one gets a big-enough stack from the pool or a fresh
        // allocation.
        let big = ByteSize::mib(4).as_u64() as usize;
        set_stack_size(big);
        assert!(on_wasm_stack(big, None, || ()).is_ok());

        let cached_size = cached_entries()[0].unwrap_or(0);
        assert!(
            cached_size >= big,
            "the thread should keep a coroutine on the bigger stack after the size bump"
        );

        clear_tls_stack();
        drain_stack_pool();
    }
}
