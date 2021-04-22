use super::{tls, unwind as do_unwind, Trap};
use std::cell::RefCell;
use std::convert::TryInto;
use std::io;
use std::mem::{self, MaybeUninit};
use std::ptr::{self, null_mut};

/// Function which may handle custom signals while processing traps.
pub type SignalHandler<'a> =
    dyn Fn(libc::c_int, *const libc::siginfo_t, *const libc::c_void) -> bool + 'a;

static mut PREV_SIGSEGV: MaybeUninit<libc::sigaction> = MaybeUninit::uninit();
static mut PREV_SIGBUS: MaybeUninit<libc::sigaction> = MaybeUninit::uninit();
static mut PREV_SIGILL: MaybeUninit<libc::sigaction> = MaybeUninit::uninit();
static mut PREV_SIGFPE: MaybeUninit<libc::sigaction> = MaybeUninit::uninit();

pub unsafe fn platform_init() {
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
    // On Unix (Darwin, FreeBSD), guard page accesses are raised as SIGBUS.
    if cfg!(target_arch = "arm") || cfg!(target_os = "freebsd") || cfg!(target_os = "macos") {
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

#[cfg(target_os = "macos")]
unsafe fn set_pc(cx: *mut libc::c_void, pc: usize, arg1: usize) {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "x86_64")] {
            let cx = &mut *(cx as *mut libc::ucontext_t);
            (*cx.uc_mcontext).__ss.__rip = pc as u64;
            (*cx.uc_mcontext).__ss.__rdi = arg1 as u64;
            // We're simulating a "pseudo-call" so we need to ensure
            // stack alignment is properly respected, notably that on a
            // `call` instruction the stack is 8/16-byte aligned, then
            // the function adjusts itself to be 16-byte aligned.
            //
            // Most of the time the stack pointer is 16-byte aligned at
            // the time of the trap but for more robust-ness with JIT
            // code where it may ud2 in a prologue check before the
            // stack is aligned we double-check here.
            if (*cx.uc_mcontext).__ss.__rsp % 16 == 0 {
                (*cx.uc_mcontext).__ss.__rsp -= 8;
            }
        } else if #[cfg(target_arch = "aarch64")] {
            let cx = &mut *(cx as *mut libc::ucontext_t);
            (*cx.uc_mcontext).__ss.__pc = pc as u64;
            (*cx.uc_mcontext).__ss.__x[0] = arg1 as u64;
        } else {
            compile_error!("unsupported macos target architecture");
        }
    }
}

unsafe extern "C" fn trap_handler(
    signum: libc::c_int,
    siginfo: *mut libc::siginfo_t,
    context: *mut libc::c_void,
) {
    println!("traphandler called");
    let previous = match signum {
        libc::SIGSEGV => &PREV_SIGSEGV,
        libc::SIGBUS => &PREV_SIGBUS,
        libc::SIGFPE => &PREV_SIGFPE,
        libc::SIGILL => &PREV_SIGILL,
        _ => panic!("unknown signal: {}", signum),
    };
    let is_stack_overflow = match signum {
        libc::SIGSEGV | libc::SIGBUS => {
            let addr = (*siginfo).si_addr() as usize;
            let (stackaddr, stacksize) = thread_stack();
            // The stack and its guard page covers the
            // range [stackaddr - guard pages .. stackaddr + stacksize).
            // We assume the guard page is 1 page, and pages are 4KiB (or 16KiB in Apple Silicon)
            stackaddr - region::page::size() <= addr && addr < stackaddr + stacksize
        }
        _ => false,
    };
    println!("IS STACKOVERFLOW: {:?}", is_stack_overflow);
    let handled = tls::with(|info| {
        println!("traphandler handled");
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
        println!("traphandler get pc");
        let pc = get_pc(context);
        let jmp_buf = info.jmp_buf_if_trap(pc, |handler| handler(signum, siginfo, context));

        // Figure out what to do based on the result of this handling of
        // the trap. Note that our sentinel value of 1 means that the
        // exception was handled by a custom exception handler, so we
        // keep executing.
        if jmp_buf.is_null() {
            println!("jumpbuf null");
            return false;
        }
        if jmp_buf as usize == 1 {
            println!("jumpbuf 1 size");
            return true;
        }
        println!("HANDLED");
        info.capture_backtrace(pc);
        println!("jumpbuf A");
        // On macOS this is a bit special. If we were to
        // `siglongjmp` out of the signal handler that notably
        // does *not* reset the sigaltstack state of our
        // signal handler. This seems to trick the kernel into
        // thinking that the sigaltstack is still in use upon
        // delivery of the next signal, meaning that the
        // sigaltstack is not ever used again if we
        // immediately call `unwind` here.
        //
        // Note that if we use `longjmp` instead of
        // `siglongjmp` then the problem is fixed. The problem
        // with that, however, is that `setjmp` is much slower
        // than `sigsetjmp` due to the preservation of the
        // proceses signal mask. The reason `longjmp` appears
        // to work is that it seems to call a function
        // (according to published macOS sources) called
        // `_sigunaltstack` which updates the kernel to say
        // the sigaltstack is no longer in use. We ideally
        // want to call that here but it's unlikely there's a
        // stable way for us to call that.
        //
        // Given all that, on macOS only, we do the next best
        // thing. We return from the signal handler after
        // updating the register context. This will cause
        // control to return to our `unwind_shim` function
        // defined here which will perform the `unwind`
        // (`siglongjmp`) for us. The reason this works is
        // that by returning from the signal handler we'll
        // trigger all the normal machinery for "the signal
        // handler is done running" which will clear the
        // sigaltstack flag and allow reusing it for the next
        // signal. Then upon resuming in our custom code we
        // blow away the stack anyway with a `longjmp`.
        // if cfg!(target_os = "macos") {
        //     unsafe extern "C" fn unwind_shim(jmp_buf: *const u8) {
        //         println!("unwind shim");
        //         do_unwind(jmp_buf)
        //     }
        //     println!("Set pc prev");
        //     set_pc(context, unwind_shim as usize, jmp_buf as usize);

        //     true
        // } else {
            do_unwind(jmp_buf)
        // }
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
        mem::transmute::<usize, extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void)>(
            previous.sa_sigaction,
        )(signum, siginfo, context)
    } else if previous.sa_sigaction == libc::SIG_DFL || previous.sa_sigaction == libc::SIG_IGN {
        libc::sigaction(signum, previous, ptr::null_mut());
    } else {
        mem::transmute::<usize, extern "C" fn(libc::c_int)>(previous.sa_sigaction)(signum)
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
        } else if #[cfg(all(any(target_os = "linux", target_os = "android"), target_arch = "aarch64"))] {
            let cx = &*(cx as *const libc::ucontext_t);
            cx.uc_mcontext.pc as *const u8
        } else if #[cfg(all(target_os = "macos", target_arch = "x86_64"))] {
            let cx = &*(cx as *const libc::ucontext_t);
            (*cx.uc_mcontext).__ss.__rip as *const u8
        } else if #[cfg(all(target_os = "macos", target_arch = "aarch64"))] {
            let cx = &*(cx as *const libc::ucontext_t);
            (*cx.uc_mcontext).__ss.__pc as *const u8
        } else if #[cfg(all(target_os = "freebsd", target_arch = "x86_64"))] {
            let cx = &*(cx as *const libc::ucontext_t);
            cx.uc_mcontext.mc_rip as *const u8
        } else {
            compile_error!("unsupported platform");
        }
    }
}

/// A function for registering a custom alternate signal stack (sigaltstack).
///
/// Rust's libstd installs an alternate stack with size `SIGSTKSZ`, which is not
/// always large enough for our signal handling code. Override it by creating
/// and registering our own alternate stack that is large enough and has a guard
/// page.
pub fn lazy_per_thread_init() -> Result<(), Trap> {
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
                Tls::Allocated {
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
