//! Code for interacting with the
//! [GDB JIT interface](https://sourceware.org/gdb/current/onlinedocs/gdb.html#JIT-Interface).

use lazy_static::lazy_static;

use std::os::raw::c_char;
use std::ptr;
use std::sync::{Arc, Mutex};

/// Entrypoint that the debugger will use to trigger a read from the
/// [`__jit_debug_descriptor`] global variable.
///
/// The debugger will wait for this function to be called and then take
/// control to read the data we prepared.
// Implementation of this function is derived from wasmtime and is licensed under
// the Apache 2.0 license.  See ATTRIBUTIONS.md for full license and more
// information.
#[cfg(not(feature = "generate-debug-information-no-export-symbols"))]
#[no_mangle]
#[inline(never)]
extern "C" fn __jit_debug_register_code() {
    // This code exists to prevent optimization of this function so that the
    // GDB JIT interface behaves as expected
    let x = 42;
    unsafe {
        std::ptr::read_volatile(&x);
    }
}

/// The operation that the debugger should perform with the entry that we gave it.
#[derive(Debug)]
#[repr(u32)]
enum JitAction {
    /// Do nothing.
    NoAction = 0,
    /// Register the given code.
    RegisterFn = 1,
    /// Unregister the given code.
    UnregisterFn = 2,
}

/// Node of the doubly linked list that the GDB JIT interface reads from.
#[repr(C)]
struct JitCodeEntry {
    /// Next entry in the linked list.
    next: *mut Self,
    /// Previous entry in the linked list.
    prev: *mut Self,
    /// Pointer to the data we want the debugger to read.
    symfile_addr: *const c_char,
    /// The amount of data at the `symfile_addr` pointer.
    symfile_size: u64,
}

impl Default for JitCodeEntry {
    fn default() -> Self {
        Self {
            next: ptr::null_mut(),
            prev: ptr::null_mut(),
            symfile_addr: ptr::null(),
            symfile_size: 0,
        }
    }
}

/// Head node of the doubly linked list that the GDB JIT interface expects.
#[no_mangle]
#[repr(C)]
struct JitDebugDescriptor {
    /// The version of the JIT interface to use.
    version: u32,
    /// Which action to perform.
    action_flag: JitAction,
    /// The entry in the list that the `action_flag` applies to.
    relevant_entry: *mut JitCodeEntry,
    /// The first entry in the doubly linked list.
    first_entry: *mut JitCodeEntry,
}

/// Global variable that the GDB JIT interface will read the data from.
/// The data is in the form of a doubly linked list. This global variable acts
/// as a head node with extra information about the operation that we want the
/// debugger to perform.
#[cfg(not(feature = "generate-debug-information-no-export-symbols"))]
#[no_mangle]
#[allow(non_upper_case_globals)]
static mut __jit_debug_descriptor: JitDebugDescriptor = JitDebugDescriptor {
    version: 1,
    action_flag: JitAction::NoAction,
    relevant_entry: ptr::null_mut(),
    first_entry: ptr::null_mut(),
};

#[cfg(feature = "generate-debug-information-no-export-symbols")]
extern "C" {
    #[no_mangle]
    static mut __jit_debug_descriptor: JitDebugDescriptor;
    #[no_mangle]
    fn __jit_debug_register_code();
}

lazy_static! {
    /// Global lock on [`__jit_debug_descriptor`]. Acquire this lock when
    /// reading or writing to the global variable. This includes calls to
    /// [`__jit_debug_register_code`] which may cause a debugger to read from
    /// the global variable.
    static ref JIT_DEBUG_DESCRIPTOR_LOCK: Mutex<()> = Mutex::new(());
}

/// Prepend an item to the front of the `__jit_debug_descriptor` entry list
///
/// # Safety
/// - Access to underlying global variable is unsynchronized: acquire a lock on
///   [`JIT_DEBUG_DESCRIPTOR_LOCK`] before calling this function.
/// - Pointer to [`JitCodeEntry`] must point to a valid entry.
unsafe fn push_front(jce: *mut JitCodeEntry) {
    if __jit_debug_descriptor.first_entry.is_null() {
        __jit_debug_descriptor.first_entry = jce;
    } else {
        let old_first = __jit_debug_descriptor.first_entry;
        assert!((*old_first).prev.is_null());
        (*jce).next = old_first;
        (*old_first).prev = jce;
        __jit_debug_descriptor.first_entry = jce;
    }
}

/// Removes an entry from the doubly linked list, updating both nodes that it's
/// connected to.
///
/// # Safety
/// - Access to underlying global variable is unsynchronized: acquire a lock on
///   [`JIT_DEBUG_DESCRIPTOR_LOCK`] before calling this function.
/// - Pointer to [`JitCodeEntry`] must point to a valid entry.
unsafe fn remove_node(jce: *mut JitCodeEntry) {
    if __jit_debug_descriptor.first_entry == jce {
        assert!((*jce).prev.is_null());
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
struct JitCodeDebugInfoEntryHandleInner(*mut JitCodeEntry);

// this is safe because the pointer is never mutated directly and then
// [`JIT_DEBUG_DESCRIPTOR_LOCK`] should always be held whenever any mutation
// can happen.
unsafe impl Send for JitCodeDebugInfoEntryHandleInner {}
unsafe impl Sync for JitCodeDebugInfoEntryHandleInner {}

/// Handle to debug info about JIT code registered with a debugger
#[derive(Debug, Clone)]
pub(crate) struct JitCodeDebugInfoEntryHandle(Arc<JitCodeDebugInfoEntryHandleInner>);

impl Drop for JitCodeDebugInfoEntryHandleInner {
    fn drop(&mut self) {
        let _guard = JIT_DEBUG_DESCRIPTOR_LOCK.lock().unwrap();
        unsafe {
            // unregister the function when dropping the JIT code entry
            __jit_debug_descriptor.relevant_entry = self.0;
            __jit_debug_descriptor.action_flag = JitAction::UnregisterFn;
            __jit_debug_register_code();
            __jit_debug_descriptor.relevant_entry = ptr::null_mut();
            __jit_debug_descriptor.action_flag = JitAction::NoAction;
            remove_node(self.0);
            let entry: Box<JitCodeEntry> = Box::from_raw(self.0);
            Vec::from_raw_parts(
                entry.symfile_addr as *mut u8,
                entry.symfile_size as _,
                entry.symfile_size as _,
            );
        }
    }
}

/// Manager of debug info registered with the debugger.
#[derive(Debug, Clone)]
pub(crate) struct JitCodeDebugInfoManager {
    inner: Vec<JitCodeDebugInfoEntryHandle>,
}

impl Default for JitCodeDebugInfoManager {
    fn default() -> Self {
        Self::new()
    }
}

impl JitCodeDebugInfoManager {
    pub(crate) fn new() -> Self {
        unsafe {
            // ensure we set the version, even if externally linked
            __jit_debug_descriptor.version = 1;
        }
        Self { inner: vec![] }
    }

    /// Register debug info relating to JIT code with the debugger.
    pub(crate) fn register_new_jit_code_entry(
        &mut self,
        bytes: &[u8],
    ) -> JitCodeDebugInfoEntryHandle {
        let mut owned_bytes = bytes.iter().cloned().collect::<Vec<u8>>();
        // ensure length == capacity to simplify memory freeing code
        owned_bytes.shrink_to_fit();
        let ptr = owned_bytes.as_mut_ptr();
        let len = owned_bytes.len();

        std::mem::forget(owned_bytes);

        let entry: *mut JitCodeEntry = Box::into_raw(Box::new(JitCodeEntry {
            symfile_addr: ptr as *const _,
            symfile_size: len as _,
            ..JitCodeEntry::default()
        }));

        unsafe {
            let _guard = JIT_DEBUG_DESCRIPTOR_LOCK.lock().unwrap();
            push_front(entry);
            __jit_debug_descriptor.relevant_entry = entry;
            __jit_debug_descriptor.action_flag = JitAction::RegisterFn;
            __jit_debug_register_code();
            __jit_debug_descriptor.relevant_entry = ptr::null_mut();
            __jit_debug_descriptor.action_flag = JitAction::NoAction;
        }

        let handle = JitCodeDebugInfoEntryHandle(Arc::new(JitCodeDebugInfoEntryHandleInner(entry)));
        self.inner.push(handle.clone());

        handle
    }
}
