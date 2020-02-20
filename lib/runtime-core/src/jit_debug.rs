//! Code for interacting with the
//! [GDB JIT inteface](https://sourceware.org/gdb/current/onlinedocs/gdb.html#JIT-Interface).

use std::ptr;
use std::sync::Arc;

/// Entrypoint that the debugger will use to trigger a read from the
/// [`__jit_debug_descriptor`] global variable.
///
/// The debugger will wait for this function to be called and then take
/// control to read the data we prepared.
// Implementation of this function is derived from wasmtime and is licensed under
// the Apache 2.0 license.  See ATTRIBUTIONS.md for full license and more
// information.
#[no_mangle]
#[inline(never)]
extern "C" fn __jit_debug_register_code() {
    // This code exists to prevent optimization of this function so that the
    // GDB JIT interface behaves as expected
    let x = 3;
    unsafe {
        std::ptr::read_volatile(&x);
    }
}

/// The operation that the debugger should perform with the entry that we gave it.
#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(u32)]
pub(crate) enum JITAction {
    /// Do nothing.
    JIT_NOACTION = 0,
    /// Register the given code.
    JIT_REGISTER_FN = 1,
    /// Unregister the given code.
    JIT_UNREGISTER_FN = 2,
}

/// Node of the doubly linked list that the GDB JIT interface reads from.
#[no_mangle]
#[repr(C)]
pub(crate) struct JITCodeEntry {
    /// Next entry in the linked list.
    next: *mut JITCodeEntry,
    /// Previous entry in the linked list.
    prev: *mut JITCodeEntry,
    /// Pointer to the data we want the debugger to read.
    symfile_addr: *mut u8,
    /// The amount of data at the `symfile_addr` pointer.
    symfile_size: u64,
}

impl Default for JITCodeEntry {
    fn default() -> Self {
        Self {
            next: ptr::null_mut(),
            prev: ptr::null_mut(),
            symfile_addr: ptr::null_mut(),
            symfile_size: 0,
        }
    }
}

/// Head node of the doubly linked list that the GDB JIT interface expects.
#[no_mangle]
#[repr(C)]
pub(crate) struct JitDebugDescriptor {
    /// The version of the JIT interface to use (presumably, TODO: double check this)
    version: u32,
    /// Which action to perform, see [`JITAction`].
    action_flag: u32,
    /// The entry in the list that the `action_flag` applies to.
    relevant_entry: *mut JITCodeEntry,
    /// The first entry in the doubly linked list.
    first_entry: *mut JITCodeEntry,
}

/// Global variable that the GDB JIT interface will read the data from.
/// The data is in the form of a doubly linked list.  This global variable acts
/// as a head node with extra information about the operation that we want the
/// debugger to perform.
#[no_mangle]
#[allow(non_upper_case_globals)]
pub(crate) static mut __jit_debug_descriptor: JitDebugDescriptor = JitDebugDescriptor {
    version: 1,
    action_flag: JITAction::JIT_NOACTION as _,
    relevant_entry: ptr::null_mut(),
    first_entry: ptr::null_mut(),
};

/// Prepend an item to the front of the `__jit_debug_descriptor` entry list
///
/// # Safety
/// - Access to underlying global variable is unsynchronized.
/// - Pointer to [`JITCodeEntry`] should point to a valid entry.
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

/// Removes an entry from the doubly linked list, updating both nodes that it's
/// connected to.
///
/// # Safety
/// - Access to underlying global variable is unsynchronized.
/// - Pointer must point to a valid `JitCodeEntry`.
unsafe fn remove_node(jce: *mut JITCodeEntry) {
    if __jit_debug_descriptor.first_entry == jce {
        debug_assert!((*jce).prev.is_null());
        __jit_debug_descriptor.first_entry = (*jce).next;
    }
    if !(*jce).prev.is_null() {
        (*(*jce).prev).next = (*jce).next;
    }
    if !(*jce).next.is_null() {
        (*(*jce).next).prev = (*jce).prev;
    }
}

/// Type for implementing Drop on the memory shared with the debugger.
#[derive(Debug)]
struct JITCodeDebugInfoEntryHandleInner(*mut JITCodeEntry);

/// Handle to debug info about JIT code registered with a debugger
#[derive(Debug, Clone)]
pub(crate) struct JITCodeDebugInfoEntryHandle(Arc<JITCodeDebugInfoEntryHandleInner>);

impl Drop for JITCodeDebugInfoEntryHandleInner {
    fn drop(&mut self) {
        unsafe {
            // unregister the function when dropping the JIT code entry
            __jit_debug_descriptor.relevant_entry = self.0;
            __jit_debug_descriptor.action_flag = JITAction::JIT_UNREGISTER_FN as u32;
            __jit_debug_register_code();
            __jit_debug_descriptor.relevant_entry = ptr::null_mut();
            __jit_debug_descriptor.action_flag = JITAction::JIT_NOACTION as u32;
            remove_node(self.0);
            let entry: Box<JITCodeEntry> = Box::from_raw(self.0);
            Vec::from_raw_parts(
                entry.symfile_addr,
                entry.symfile_size as _,
                entry.symfile_size as _,
            );
        }
    }
}

/// Manager of debug info registered with the debugger.
#[derive(Debug, Clone, Default)]
pub struct JITCodeDebugInfoManager {
    inner: Vec<JITCodeDebugInfoEntryHandle>,
}

impl JITCodeDebugInfoManager {
    /// Register debug info relating to JIT code with the debugger.
    pub(crate) fn register_new_jit_code_entry(
        &mut self,
        bytes: &[u8],
    ) -> JITCodeDebugInfoEntryHandle {
        let mut owned_bytes = bytes.iter().cloned().collect::<Vec<u8>>();
        // ensure length == capacity to simplify memory freeing code
        owned_bytes.shrink_to_fit();
        let ptr = owned_bytes.as_mut_ptr();
        let len = owned_bytes.len();

        std::mem::forget(owned_bytes);

        let entry: *mut JITCodeEntry = Box::into_raw(Box::new(JITCodeEntry {
            symfile_addr: ptr,
            symfile_size: len as _,
            ..JITCodeEntry::default()
        }));

        unsafe {
            push_front(entry);
            __jit_debug_descriptor.relevant_entry = entry;
            __jit_debug_descriptor.action_flag = JITAction::JIT_REGISTER_FN as u32;
            __jit_debug_register_code();
            __jit_debug_descriptor.relevant_entry = ptr::null_mut();
            __jit_debug_descriptor.action_flag = JITAction::JIT_NOACTION as u32;
        }

        let handle = JITCodeDebugInfoEntryHandle(Arc::new(JITCodeDebugInfoEntryHandleInner(entry)));
        self.inner.push(handle.clone());

        handle
    }
}
