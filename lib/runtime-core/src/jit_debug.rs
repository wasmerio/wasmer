#![allow(missing_docs)]
use std::ptr;

// =============================================================================
// LLDB hook magic:
// see lldb/packages/Python/lldbsuite/test/functionalities/jitloader_gdb in
// llvm repo for example
//
// see also https://sourceware.org/gdb/current/onlinedocs/gdb.html#JIT-Interface

#[no_mangle]
#[inline(never)]
extern "C" fn __jit_debug_register_code() {
    // implementation of this function copied from wasmtime (TODO: link and attribution)
    // prevent optimization of this function
    let x = 3;
    unsafe {
        std::ptr::read_volatile(&x);
    }
}

#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(u32)]
pub enum JITAction {
    JIT_NOACTION = 0,
    JIT_REGISTER_FN = 1,
    JIT_UNREGISTER_FN = 2,
}

#[no_mangle]
#[repr(C)]
pub struct JITCodeEntry {
    next: *mut JITCodeEntry,
    prev: *mut JITCodeEntry,
    // TODO: use CStr here?
    symfile_addr: *const u8,
    symfile_size: u64,
}

impl Default for JITCodeEntry {
    fn default() -> Self {
        Self {
            next: ptr::null_mut(),
            prev: ptr::null_mut(),
            symfile_addr: ptr::null(),
            symfile_size: 0,
        }
    }
}

#[no_mangle]
#[repr(C)]
pub struct JitDebugDescriptor {
    version: u32,
    action_flag: u32,
    relevant_entry: *mut JITCodeEntry,
    first_entry: *mut JITCodeEntry,
}

#[no_mangle]
#[allow(non_upper_case_globals)]
pub static mut __jit_debug_descriptor: JitDebugDescriptor = JitDebugDescriptor {
    version: 1,
    action_flag: JITAction::JIT_NOACTION as _,
    relevant_entry: ptr::null_mut(),
    first_entry: ptr::null_mut(),
};

/// Prepend an item to the front of the `__jit_debug_descriptor` entry list
///
/// # Safety
/// - Pointer to [`JITCodeEntry`] should point to a valid entry and stay alive
///   for the 'static lifetime
unsafe fn push_front(jce: *mut JITCodeEntry) {
    if __jit_debug_descriptor.first_entry.is_null() {
        __jit_debug_descriptor.first_entry = jce;
    } else {
        let old_first = __jit_debug_descriptor.first_entry;
        debug_assert!((*old_first).prev.is_null());
        (*jce).next = old_first;
        (*old_first).prev = jce;
        __jit_debug_descriptor.first_entry = jce;
    }
}

// deleted static (added and deleted by Mark): TODO:
pub fn register_new_jit_code_entry(bytes: &[u8], action: JITAction) -> *mut JITCodeEntry {
    let owned_bytes = bytes.iter().cloned().collect::<Vec<u8>>();
    let ptr = owned_bytes.as_ptr();
    let len = owned_bytes.len();

    std::mem::forget(bytes);

    let entry: *mut JITCodeEntry = Box::into_raw(Box::new(JITCodeEntry {
        symfile_addr: ptr,
        symfile_size: len as _,
        ..JITCodeEntry::default()
    }));

    unsafe {
        push_front(entry);
        __jit_debug_descriptor.relevant_entry = entry;
        __jit_debug_descriptor.action_flag = action as u32;
        __jit_debug_register_code();
        __jit_debug_descriptor.relevant_entry = ptr::null_mut();
        __jit_debug_descriptor.action_flag = JITAction::JIT_NOACTION as _;
    }

    entry
}
