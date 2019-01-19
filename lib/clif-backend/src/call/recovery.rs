//! When a WebAssembly module triggers any traps, we perform recovery here.
//!
//! This module uses TLS (thread-local storage) to track recovery information. Since the four signals we're handling
//! are very special, the async signal unsafety of Rust's TLS implementation generally does not affect the correctness here
//! unless you have memory unsafety elsewhere in your code.

use crate::call::sighandler::install_sighandler;
use crate::relocation::{TrapData, TrapSink};
use cranelift_codegen::ir::TrapCode;
use nix::libc::{c_void, siginfo_t};
use nix::sys::signal::{Signal, SIGBUS, SIGFPE, SIGILL, SIGSEGV};
use std::cell::{Cell, UnsafeCell};
use std::ptr;
use std::sync::Once;
use wasmer_runtime::{
    error::{RuntimeError, RuntimeResult},
    structures::TypedIndex,
    types::{MemoryIndex, TableIndex},
};

extern "C" {
    pub fn setjmp(env: *mut ::nix::libc::c_void) -> ::nix::libc::c_int;
    fn longjmp(env: *mut ::nix::libc::c_void, val: ::nix::libc::c_int) -> !;
}

const SETJMP_BUFFER_LEN: usize = 27;
pub static SIGHANDLER_INIT: Once = Once::new();

thread_local! {
    pub static SETJMP_BUFFER: UnsafeCell<[::nix::libc::c_int; SETJMP_BUFFER_LEN]> = UnsafeCell::new([0; SETJMP_BUFFER_LEN]);
    pub static CAUGHT_ADDRESSES: Cell<(*const c_void, *const c_void)> = Cell::new((ptr::null(), ptr::null()));
    pub static CURRENT_EXECUTABLE_BUFFER: Cell<*const c_void> = Cell::new(ptr::null());
}

pub struct HandlerData {
    trap_data: TrapSink,
    buffer_ptr: *const c_void,
    buffer_size: usize,
}

impl HandlerData {
    pub fn new(trap_data: TrapSink, buffer_ptr: *const c_void, buffer_size: usize) -> Self {
        Self {
            trap_data,
            buffer_ptr,
            buffer_size,
        }
    }

    pub fn lookup(&self, ip: *const c_void) -> Option<TrapData> {
        let ip = ip as usize;
        let buffer_ptr = self.buffer_ptr as usize;

        if buffer_ptr <= ip && ip < buffer_ptr + self.buffer_size {
            let offset = ip - buffer_ptr;
            self.trap_data.lookup(offset)
        } else {
            None
        }
    }
}

pub fn call_protected<T>(handler_data: &HandlerData, f: impl FnOnce() -> T) -> RuntimeResult<T> {
    unsafe {
        let jmp_buf = SETJMP_BUFFER.with(|buf| buf.get());
        let prev_jmp_buf = *jmp_buf;

        SIGHANDLER_INIT.call_once(|| {
            install_sighandler();
        });

        let signum = setjmp(jmp_buf as *mut ::nix::libc::c_void);
        if signum != 0 {
            *jmp_buf = prev_jmp_buf;
            let (faulting_addr, _) = CAUGHT_ADDRESSES.with(|cell| cell.get());

            if let Some(TrapData {
                trapcode,
                srcloc: _,
            }) = handler_data.lookup(faulting_addr)
            {
                Err(match Signal::from_c_int(signum) {
                    Ok(SIGILL) => match trapcode {
                        TrapCode::BadSignature => RuntimeError::IndirectCallSignature {
                            table: TableIndex::new(0),
                        },
                        TrapCode::IndirectCallToNull => RuntimeError::IndirectCallToNull {
                            table: TableIndex::new(0),
                        },
                        TrapCode::HeapOutOfBounds => RuntimeError::OutOfBoundsAccess {
                            memory: MemoryIndex::new(0),
                            addr: 0,
                        },
                        TrapCode::TableOutOfBounds => RuntimeError::TableOutOfBounds {
                            table: TableIndex::new(0),
                        },
                        _ => RuntimeError::Unknown {
                            msg: "unknown trap".to_string(),
                        },
                    },
                    Ok(SIGSEGV) | Ok(SIGBUS) => {
                        // I'm too lazy right now to actually check if the address is within one of the memories,
                        // so just say that it's a memory-out-of-bounds access for now
                        RuntimeError::OutOfBoundsAccess {
                            memory: MemoryIndex::new(0),
                            addr: 0,
                        }
                    }
                    Ok(SIGFPE) => RuntimeError::IllegalArithmeticOperation,
                    _ => unimplemented!(),
                }
                .into())
            } else {
                let signal = match Signal::from_c_int(signum) {
                    Ok(SIGFPE) => "floating-point exception",
                    Ok(SIGILL) => "illegal instruction",
                    Ok(SIGSEGV) => "segmentation violation",
                    Ok(SIGBUS) => "bus error",
                    Err(_) => "error while getting the Signal",
                    _ => "unkown trapped signal",
                };
                // When the trap-handler is fully implemented, this will return more information.
                Err(RuntimeError::Unknown {
                    msg: format!("trap at {:p} - {}", faulting_addr, signal),
                }
                .into())
            }
        } else {
            let ret = f(); // TODO: Switch stack?
            *jmp_buf = prev_jmp_buf;
            Ok(ret)
        }
    }
}

/// Unwinds to last protected_call.
pub unsafe fn do_unwind(signum: i32, siginfo: *mut siginfo_t, ucontext: *const c_void) -> ! {
    // Since do_unwind is only expected to get called from WebAssembly code which doesn't hold any host resources (locks etc.)
    // itself, accessing TLS here is safe. In case any other code calls this, it often indicates a memory safety bug and you should
    // temporarily disable the signal handlers to debug it.

    let jmp_buf = SETJMP_BUFFER.with(|buf| buf.get());
    if *jmp_buf == [0; SETJMP_BUFFER_LEN] {
        ::std::process::abort();
    }

    CAUGHT_ADDRESSES.with(|cell| cell.set(get_faulting_addr_and_ip(siginfo, ucontext)));

    longjmp(jmp_buf as *mut ::nix::libc::c_void, signum)
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
unsafe fn get_faulting_addr_and_ip(
    siginfo: *mut siginfo_t,
    _ucontext: *const c_void,
) -> (*const c_void, *const c_void) {
    (ptr::null(), ptr::null())
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
unsafe fn get_faulting_addr_and_ip(
    siginfo: *mut siginfo_t,
    _ucontext: *const c_void,
) -> (*const c_void, *const c_void) {
    ((*siginfo).si_addr, ptr::null())
}

#[cfg(not(any(
    all(target_os = "macos", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64"),
)))]
compile_error!("This crate doesn't yet support compiling on operating systems other than linux and macos and architectures other than x86_64");
