// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

//! macOS-specific handling of handling exceptions
//!
//! Unlike other Unix platforms macOS here uses mach ports to handle exceptions
//! instead of signals. While macOS platforms could use signals (and
//! historically they did!) this is incompatible when Wasmer is linked into a
//! project that is otherwise using mach ports for catching exceptions.
//!
//! Mach ports are somewhat obscure and not really heavily used in a ton of
//! places. Needless to say the original author of this file worked with mach
//! ports for the first time when writing this file. As such the exact specifics
//! here may not be super well documented. This file is 100% lifted from
//! SpiderMonkey and then adapted for Wasmer's purposes. Credit for almost
//! all of this file goes to SpiderMonkey for figuring out all the fiddly bits.
//! See also
//! <https://searchfox.org/mozilla-central/source/js/src/wasm/WasmSignalHandlers.cpp>
//! for the original code.
//!
//! The high-level overview is that when using mach ports a thread is blocked
//! when it generates an exception and then a message can be read from the
//! port. This means that, unlike signals, threads can't fix their own traps.
//! Instead a helper thread is spun up to service exception messages. This is
//! also in conflict with Wasmer's exception handling currently which is to
//! use a thread-local to store information about how to unwind. Additionally
//! this requires that the check of whether a pc is a wasm trap or not is a
//! global check rather than a per-thread check. This necessitates the existence
//! of `GlobalFrameInfo` in the `wasmer_engine` crate.
//!
//! Otherwise this file heavily uses the `mach` Rust crate for type and
//! function declarations. Many bits and pieces are copied or translated from
//! the SpiderMonkey implementation and it should pass all the tests!

#![allow(non_snake_case)]

use super::{tls, unwind as do_unwind, Trap};
use mach::exception_types::*;
use mach::kern_return::*;
use mach::mach_init::*;
use mach::mach_port::*;
use mach::message::*;
use mach::port::*;
use mach::thread_act::*;
use mach::traps::*;
use std::cell::Cell;
use std::mem;
use std::thread;

/// Other `mach` declarations awaiting <https://github.com/fitzgen/mach/pull/64>
/// to be merged.
mod mach_addons {
    #![allow(non_camel_case_types)]
    #![allow(non_upper_case_globals)]
    #![allow(dead_code)]

    use mach::{
        exception_types::*, kern_return::*, mach_types::*, message::*, port::*, thread_status::*,
    };
    use std::mem;

    #[repr(C)]
    #[derive(Copy, Clone, Debug)]
    #[allow(dead_code)]
    pub struct NDR_record_t {
        mig_vers: libc::c_uchar,
        if_vers: libc::c_uchar,
        reserved1: libc::c_uchar,
        mig_encoding: libc::c_uchar,
        int_rep: libc::c_uchar,
        char_rep: libc::c_uchar,
        float_rep: libc::c_uchar,
        reserved32: libc::c_uchar,
    }

    extern "C" {
        pub static NDR_record: NDR_record_t;
    }

    #[repr(C)]
    #[allow(dead_code)]
    #[derive(Copy, Clone, Debug)]
    pub struct __Request__exception_raise_t {
        pub Head: mach_msg_header_t,
        /* start of the kernel processed data */
        pub msgh_body: mach_msg_body_t,
        pub thread: mach_msg_port_descriptor_t,
        pub task: mach_msg_port_descriptor_t,
        /* end of the kernel processed data */
        pub NDR: NDR_record_t,
        pub exception: exception_type_t,
        pub codeCnt: mach_msg_type_number_t,
        pub code: [i64; 2],
    }

    #[repr(C)]
    #[allow(dead_code)]
    #[derive(Copy, Clone, Debug)]
    pub struct __Reply__exception_raise_t {
        pub Head: mach_msg_header_t,
        pub NDR: NDR_record_t,
        pub RetCode: kern_return_t,
    }

    #[repr(C)]
    #[derive(Copy, Clone, Debug, Default, Hash, PartialOrd, PartialEq, Eq, Ord)]
    pub struct arm_thread_state64_t {
        pub __x: [u64; 29],
        pub __fp: u64, // frame pointer x29
        pub __lr: u64, // link register x30
        pub __sp: u64, // stack pointer x31
        pub __pc: u64,
        pub __cpsr: u32,
        pub __pad: u32,
    }

    impl arm_thread_state64_t {
        pub fn count() -> mach_msg_type_number_t {
            (mem::size_of::<Self>() / mem::size_of::<u32>()) as mach_msg_type_number_t
        }
    }

    pub static ARM_THREAD_STATE64: thread_state_flavor_t = 6;

    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    pub static THREAD_STATE_NONE: thread_state_flavor_t = 13;
    #[cfg(target_arch = "aarch64")]
    pub static THREAD_STATE_NONE: thread_state_flavor_t = 5;

    extern "C" {
        pub fn thread_set_state(
            target_act: thread_port_t,
            flavor: thread_state_flavor_t,
            new_state: thread_state_t,
            new_stateCnt: mach_msg_type_number_t,
        ) -> kern_return_t;

        pub fn thread_set_exception_ports(
            thread: thread_port_t,
            exception_mask: exception_mask_t,
            new_port: mach_port_t,
            behavior: libc::c_uint,
            new_flavor: thread_state_flavor_t,
        ) -> kern_return_t;
    }
}

use mach_addons::*;

/// Just used below
pub enum Void {}
/// For now this is basically unused, we don't expose this any more for
/// Wasmer on macOS.
pub type SignalHandler<'a> = dyn Fn(Void) -> bool + 'a;

/// Process-global port that we use to route thread-level exceptions to.
static mut WASMER_PORT: mach_port_name_t = MACH_PORT_NULL;

pub unsafe fn platform_init() {
    // Allocate our WASMER_PORT and make sure that it can be sent to so we
    // can receive exceptions.
    let me = mach_task_self();
    let kret = mach_port_allocate(me, MACH_PORT_RIGHT_RECEIVE, &mut WASMER_PORT);
    assert_eq!(kret, KERN_SUCCESS, "failed to allocate port");
    let kret = mach_port_insert_right(me, WASMER_PORT, WASMER_PORT, MACH_MSG_TYPE_MAKE_SEND);
    assert_eq!(kret, KERN_SUCCESS, "failed to insert right");

    // Spin up our handler thread which will solely exist to service exceptions
    // generated by other threads. Note that this is a background thread that
    // we're not very interested in so it's detached here.
    thread::spawn(|| handler_thread());
}

// This is largely just copied from SpiderMonkey.
#[repr(C)]
#[allow(dead_code)]
struct ExceptionRequest {
    body: __Request__exception_raise_t,
    trailer: mach_msg_trailer_t,
}

unsafe fn handler_thread() {
    // Taken from mach_exc in /usr/include/mach/mach_exc.defs.
    const EXCEPTION_MSG_ID: mach_msg_id_t = 2405;

    loop {
        // Block this thread reading a message from our port. This will block
        // until some thread throws an exception. Note that messages are all
        // expected to be exceptions here.
        let mut request: ExceptionRequest = mem::zeroed();
        let kret = mach_msg(
            &mut request.body.Head,
            MACH_RCV_MSG,
            0,
            mem::size_of_val(&request) as u32,
            WASMER_PORT,
            MACH_MSG_TIMEOUT_NONE,
            MACH_PORT_NULL,
        );
        if kret != KERN_SUCCESS {
            eprintln!("mach_msg failed with {} ({0:x})", kret);
            libc::abort();
        }
        if request.body.Head.msgh_id != EXCEPTION_MSG_ID {
            eprintln!("unexpected msg header id {}", request.body.Head.msgh_id);
            libc::abort();
        }

        // Attempt to handle the exception below which will process the state
        // of the request.
        //
        // We unconditionally need to send a message back on our port after
        // this exception is received, and our reply code here dictates whether
        // the thread crashes or whether we continue execution of the thread.
        let reply_code = if handle_exception(&mut request) {
            KERN_SUCCESS
        } else {
            KERN_FAILURE
        };

        // This magic incantation to send a reply back to the kernel was
        // derived from the exc_server generated by
        // 'mig -v /usr/include/mach/mach_exc.defs'.
        let mut reply: __Reply__exception_raise_t = mem::zeroed();
        reply.Head.msgh_bits =
            MACH_MSGH_BITS(request.body.Head.msgh_bits & MACH_MSGH_BITS_REMOTE_MASK, 0);
        reply.Head.msgh_size = mem::size_of_val(&reply) as u32;
        reply.Head.msgh_remote_port = request.body.Head.msgh_remote_port;
        reply.Head.msgh_local_port = MACH_PORT_NULL;
        reply.Head.msgh_id = request.body.Head.msgh_id + 100;
        reply.NDR = NDR_record;
        reply.RetCode = reply_code;
        mach_msg(
            &mut reply.Head,
            MACH_SEND_MSG,
            mem::size_of_val(&reply) as u32,
            0,
            MACH_PORT_NULL,
            MACH_MSG_TIMEOUT_NONE,
            MACH_PORT_NULL,
        );
    }
}

unsafe fn handle_exception(request: &mut ExceptionRequest) -> bool {
    println!("Handle exception: {}", request.body.exception);
    // First make sure that this exception is one that we actually expect to
    // get raised by wasm code. All other exceptions we safely ignore.
    match request.body.exception as u32 {
        EXC_BAD_ACCESS | EXC_BAD_INSTRUCTION => {}
        _ => {
            println!("RETURNED");
            return false;
        },
    }

    // Depending on the current architecture various bits and pieces of this
    // will change. This is expected to get filled out for other macos
    // platforms as necessary.
    //
    // The variables this needs to define are:
    //
    // * `ThreadState` - a structure read via `thread_get_state` to learn about
    //   the register state of the thread that trapped.
    // * `thread_state_flavor` - used to read `ThreadState`
    // * `get_pc` - a function from `&ThreadState` to a pointer to read the
    //   current program counter, used to test if it's an address we're
    //   catching wasm traps for.
    // * `resume` - a function used to modify `ThreadState` to resume in the
    //   target thread in the `unwind` function below, passing the two
    //   parameters as the first two arguments.
    // * `thread_state` - a fresh instance of `ThreadState` to read into
    // * `thread_state_count` - the size to pass to `mach_msg`.
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "x86_64")] {
            use mach::structs::x86_thread_state64_t;
            use mach::thread_status::x86_THREAD_STATE64;

            type ThreadState = x86_thread_state64_t;

            let thread_state_flavor = x86_THREAD_STATE64;

            let get_pc = |state: &ThreadState| state.__rip as *const u8;

            let resume = |state: &mut ThreadState, pc: usize| {
                // The x86_64 ABI requires a 16-byte stack alignment for
                // functions, so typically we'll be 16-byte aligned. In this
                // case we simulate a `call` instruction by decrementing the
                // stack pointer and pushing the "return" address which in this
                // case is the faulting address. This should help the native
                // unwinder figure out how to find the precisely trapping
                // function.
                //
                // Note, however, that if the stack is not 16-byte aligned then
                // we don't do anything. Currently this only arises due to
                // `ud2` in the prologue of functions when performing the
                // initial stack check. In the old backend 0 stack manipulation
                // happens until after the stack check passes, so if the stack
                // check fails (hence we're running in this handler) then the
                // stack is not 16-byte aligned due to the previous return
                // address pushed by `call`. In this scenario we just blow away
                // the stack frame by overwriting %rip. This technically loses
                // the precise frame that was interrupted, but that's probably
                // not the end of the world anyway.
                if state.__rsp % 16 == 0 {
                    state.__rsp -= 8;
                    *(state.__rsp as *mut u64) = state.__rip;
                }
                state.__rip = unwind as u64;
                state.__rdi = pc as u64;
            };
            let mut thread_state = ThreadState::new();
        } else if #[cfg(target_arch = "aarch64")] {
            type ThreadState = arm_thread_state64_t;

            let thread_state_flavor = ARM_THREAD_STATE64;

            let get_pc = |state: &ThreadState| state.__pc as *const u8;

            let resume = |state: &mut ThreadState, pc: usize| {
                // Clobber LR with the faulting PC, so unwinding resumes at the
                // faulting instruction. The previous value of LR has been saved
                // by the callee (in Cranelift generated code), so no need to
                // stash it.
                state.__lr = pc as u64;

                // Fill in the argument to unwind here, and set PC to it, so
                // it looks like a call to unwind.
                state.__x[0] = pc as u64;
                state.__pc = unwind as u64;
            };
            let mut thread_state = mem::zeroed::<ThreadState>();
        } else {
            compile_error!("unsupported target architecture");
        }
    }

    // First up read our origin thread's state into the area defined above.
    let origin_thread = request.body.thread.name;
    let mut thread_state_count = ThreadState::count();
    let kret = thread_get_state(
        origin_thread,
        thread_state_flavor,
        &mut thread_state as *mut ThreadState as *mut u32,
        &mut thread_state_count,
    );
    if kret != KERN_SUCCESS {
        return false;
    }

    // Use our global map to determine if this program counter is indeed a wasm
    // trap, loading the `jmp_buf` to unwind to if it is.
    //
    // Note that this is where things are pretty tricky. We're accessing
    // non-`Send` state (`CallThreadState`) from the exception handling thread.
    // While typically invalid we are guaranteed that the original thread is
    // stopped while we're accessing it here so this should be safe.
    //
    // Note also that we access the `state` outside the lock of `MAP`. This
    // again is safe because if `state` is `Some` then we're guaranteed the
    // thread is stopped and won't be removing or invalidating its state.
    // Finally our indirection with a pointer means that we can read the
    // pointer value and if `MAP` changes happen after we read our entry that's
    // ok since they won't invalidate our entry.
    let pc = get_pc(&thread_state);
    if !super::IS_WASM_PC(pc as usize) {
        return false;
    }
    println!("IS WASM TRAP!");

    // We have determined that this is a wasm trap and we need to actually
    // force the thread itself to trap. The thread's register state is
    // configured to resume in the `unwind` function below, we update the
    // thread's register state, and then we're off to the races.
    resume(&mut thread_state, pc as usize);
    let kret = thread_set_state(
        origin_thread,
        thread_state_flavor,
        &mut thread_state as *mut ThreadState as *mut u32,
        thread_state_count,
    );
    println!("KRET {} {}", kret, KERN_SUCCESS);
    kret == KERN_SUCCESS
}

/// This is a "landing pad" which is never called directly but is directly
/// resumed into from wasm-trapped threads.
///
/// This is a small shim which primarily serves the purpose of simply capturing
/// a native backtrace once we've switched back to the thread itself. After
/// the backtrace is captured we can do the usual `longjmp` back to the source
/// of the wasm code.
unsafe extern "C" fn unwind(wasm_pc: *const u8) -> ! {
    println!("DO UNWIND 0");
    let jmp_buf = tls::with(|state| {
        println!("DO UNWIND 1");
        let state = state.unwrap();
        println!("DO UNWIND 2");
        state.capture_backtrace(wasm_pc);
        println!("DO UNWIND 3");
        state.jmp_buf.get()
    });
    debug_assert!(!jmp_buf.is_null());
    println!("DO UNWIND 4");
    do_unwind(jmp_buf);
}

thread_local! {
    static MY_PORT: ClosePort = ClosePort(unsafe { mach_thread_self() });
}

struct ClosePort(mach_port_name_t);

impl Drop for ClosePort {
    fn drop(&mut self) {
        unsafe {
            mach_port_deallocate(mach_task_self(), self.0);
        }
    }
}

/// Exceptions on macOS can be delivered to either thread-level or task-level
/// exception ports. In wasmer we choose to send the exceptions to
/// thread-level ports. This means that we need to, for each thread that can
/// generate an exception, register our thread's exception port as
/// `WASMER_PORT` above.
///
/// Note that this choice is done because at the current time if we were to
/// implement a task-level (process-wide) port we'd have to figure out how to
/// forward exceptions that we're not interested to the previously registered
/// port. At this time the author isn't sure how to even do that. SpiderMonkey
/// calls this forwarding "dark magic" as well, and since SpiderMonkey chooses
/// thread-level ports then I hope that's good enough for wasmer.
///
/// Also note that this choice of thread-level ports should be fine in that
/// unhandled thread-level exceptions get automatically forwarded to the
/// task-level port which is where we'd expected things like breakpad/crashpad
/// exception handlers to get registered.
pub fn lazy_per_thread_init() -> Result<(), Trap> {
    thread_local! {
        static PORTS_SET: Cell<bool> = Cell::new(false);
    }

    PORTS_SET.with(|ports| {
        if ports.replace(true) {
            return;
        }

        unsafe {
            assert!(WASMER_PORT != MACH_PORT_NULL);
            let kret = thread_set_exception_ports(
                MY_PORT.with(|p| p.0),
                EXC_MASK_BAD_ACCESS | EXC_MASK_BAD_INSTRUCTION,
                WASMER_PORT,
                EXCEPTION_DEFAULT | MACH_EXCEPTION_CODES,
                mach_addons::THREAD_STATE_NONE,
            );
            assert_eq!(kret, KERN_SUCCESS, "failed to set thread exception port");
        }
    });
    Ok(())
}
