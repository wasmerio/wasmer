use super::common::round_up_to_page_size;
use crate::structs::{LLVMResult, MemProtect};
use std::ptr;

use winapi::um::memoryapi::{VirtualAlloc, VirtualFree};
use winapi::um::winnt::{
    MEM_COMMIT, MEM_DECOMMIT, MEM_RESERVE, PAGE_EXECUTE_READ, PAGE_NOACCESS, PAGE_READONLY,
    PAGE_READWRITE,
};

pub unsafe fn visit_fde(_addr: *mut u8, _size: usize, _visitor: extern "C" fn(*mut u8)) {
    // Do nothing on Windows
}

pub unsafe fn install_signal_handler() {
    // Do nothing on Windows
}

pub unsafe fn alloc_memory(
    size: usize,
    protect: MemProtect,
    ptr_out: &mut *mut u8,
    size_out: &mut usize,
) -> LLVMResult {
    let size = round_up_to_page_size(size);
    let flags = if protect == MemProtect::NONE {
        MEM_RESERVE
    } else {
        MEM_RESERVE | MEM_COMMIT
    };
    let ptr = VirtualAlloc(
        ptr::null_mut(),
        size,
        flags,
        memprotect_to_protect_const(protect),
    );

    if ptr.is_null() {
        return LLVMResult::ALLOCATE_FAILURE;
    }

    *ptr_out = ptr as _;
    *size_out = size;
    LLVMResult::OK
}

pub unsafe fn protect_memory(ptr: *mut u8, size: usize, protect: MemProtect) -> LLVMResult {
    let size = round_up_to_page_size(size);
    let ptr = VirtualAlloc(
        ptr as _,
        size,
        MEM_COMMIT,
        memprotect_to_protect_const(protect),
    );

    if ptr.is_null() {
        LLVMResult::PROTECT_FAILURE
    } else {
        LLVMResult::OK
    }
}

pub unsafe fn dealloc_memory(ptr: *mut u8, size: usize) -> LLVMResult {
    let success = VirtualFree(ptr as _, size, MEM_DECOMMIT);
    // If the function succeeds, the return value is nonzero.
    if success == 1 {
        LLVMResult::OK
    } else {
        LLVMResult::DEALLOC_FAILURE
    }
}

fn memprotect_to_protect_const(protect: MemProtect) -> u32 {
    match protect {
        MemProtect::NONE => PAGE_NOACCESS,
        MemProtect::READ => PAGE_READONLY,
        MemProtect::READ_WRITE => PAGE_READWRITE,
        MemProtect::READ_EXECUTE => PAGE_EXECUTE_READ,
    }
}
