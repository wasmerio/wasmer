use super::common::round_up_to_page_size;
use crate::structs::{LLVMResult, MemProtect};
use libc::{
    c_void, mmap, mprotect, munmap, siginfo_t, MAP_ANON, MAP_PRIVATE, PROT_EXEC, PROT_NONE,
    PROT_READ, PROT_WRITE,
};
use nix::sys::signal::{
    sigaction, SaFlags, SigAction, SigHandler, SigSet, SIGBUS, SIGILL, SIGSEGV,
};
use std::ptr;

/// `__register_frame` and `__deregister_frame` on macos take a single fde as an
/// argument, so we need to parse the fde table here.
///
/// This is a pretty direct port of llvm's fde handling code:
///     https://llvm.org/doxygen/RTDyldMemoryManager_8cpp_source.html.
#[allow(clippy::cast_ptr_alignment)]
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
pub unsafe fn visit_fde(addr: *mut u8, _size: usize, visitor: extern "C" fn(*mut u8)) {
    visitor(addr);
}

extern "C" {
    #[cfg_attr(nightly, unwind(allowed))]
    fn throw_trap(ty: i32) -> !;
}

pub unsafe fn install_signal_handler() {
    let sa = SigAction::new(
        SigHandler::SigAction(signal_trap_handler),
        SaFlags::SA_ONSTACK | SaFlags::SA_SIGINFO,
        SigSet::empty(),
    );
    sigaction(SIGSEGV, &sa).unwrap();
    sigaction(SIGBUS, &sa).unwrap();
    sigaction(SIGILL, &sa).unwrap();
}

#[cfg_attr(nightly, unwind(allowed))]
extern "C" fn signal_trap_handler(
    _signum: ::nix::libc::c_int,
    _siginfo: *mut siginfo_t,
    _ucontext: *mut c_void,
) {
    unsafe {
        if SigSet::all().thread_unblock().is_err() {
            std::process::abort();
        }
        // Apparently, we can unwind from arbitary instructions, as long
        // as we don't need to catch the exception inside the function that
        // was interrupted.
        //
        // This works on macos, not sure about linux.
        throw_trap(2);
    }
}

pub unsafe fn alloc_memory(
    size: usize,
    protect: MemProtect,
    ptr_out: &mut *mut u8,
    size_out: &mut usize,
) -> LLVMResult {
    let size = round_up_to_page_size(size);
    let ptr = mmap(
        ptr::null_mut(),
        size,
        match protect {
            MemProtect::NONE => PROT_NONE,
            MemProtect::READ => PROT_READ,
            MemProtect::READ_WRITE => PROT_READ | PROT_WRITE,
            MemProtect::READ_EXECUTE => PROT_READ | PROT_EXEC,
        },
        MAP_PRIVATE | MAP_ANON,
        -1,
        0,
    );
    if ptr as isize == -1 {
        return LLVMResult::ALLOCATE_FAILURE;
    }
    *ptr_out = ptr as _;
    *size_out = size;
    LLVMResult::OK
}

pub unsafe fn protect_memory(ptr: *mut u8, size: usize, protect: MemProtect) -> LLVMResult {
    let res = mprotect(
        ptr as _,
        round_up_to_page_size(size),
        match protect {
            MemProtect::NONE => PROT_NONE,
            MemProtect::READ => PROT_READ,
            MemProtect::READ_WRITE => PROT_READ | PROT_WRITE,
            MemProtect::READ_EXECUTE => PROT_READ | PROT_EXEC,
        },
    );

    if res == 0 {
        LLVMResult::OK
    } else {
        LLVMResult::PROTECT_FAILURE
    }
}

pub unsafe fn dealloc_memory(ptr: *mut u8, size: usize) -> LLVMResult {
    let res = munmap(ptr as _, round_up_to_page_size(size));

    if res == 0 {
        LLVMResult::OK
    } else {
        LLVMResult::DEALLOC_FAILURE
    }
}
