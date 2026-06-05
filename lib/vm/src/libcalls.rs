// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! Runtime library calls.
//!
//! Note that Wasm compilers may sometimes perform these inline rather than
//! calling them, particularly when CPUs have special instructions which compute
//! them directly.
//!
//! These functions are called by compiled Wasm code, and therefore must take
//! certain care about some things:
//!
//! * They must always be `pub extern "C"` and should only contain basic, raw
//!   i32/i64/f32/f64/pointer parameters that are safe to pass across the system
//!   ABI!
//!
//! * If any nested function propagates an `Err(trap)` out to the library
//!   function frame, we need to raise it. This involves some nasty and quite
//!   unsafe code under the covers! Notable, after raising the trap, drops
//!   **will not** be run for local variables! This can lead to things like
//!   leaking `VMInstance`s which leads to never deallocating JIT code,
//!   instances, and modules! Therefore, always use nested blocks to ensure
//!   drops run before raising a trap:
//!
//!   ```ignore
//!   pub extern "C" fn my_lib_function(...) {
//!       let result = {
//!           // Do everything in here so drops run at the end of the block.
//!           ...
//!       };
//!       if let Err(trap) = result {
//!           // Now we can safely raise the trap without leaking!
//!           raise_lib_trap(trap);
//!       }
//!   }
//!   ```

#![allow(missing_docs)] // For some reason lint fails saying that `LibCall` is not documented, when it actually is

use std::{ffi::c_void, panic};
mod eh;

use crate::trap::{Trap, TrapCode, raise_lib_trap};
use crate::vmcontext::VMContext;
use crate::{
    InternalStoreHandle,
    table::{RawTableElement, TableElement},
};
use crate::{VMExceptionObj, probestack::PROBESTACK};
use crate::{VMFuncRef, on_host_stack};
pub use eh::{throw, wasmer_eh_personality, wasmer_eh_personality2};
pub use wasmer_types::LibCall;
use wasmer_types::{
    DataIndex, ElemIndex, FunctionIndex, LocalMemoryIndex, LocalTableIndex, MemoryIndex, RawValue,
    TableIndex, TagIndex, Type,
};

/// Implementation of f32.ceil
#[unsafe(no_mangle)]
pub extern "C" fn wasmer_vm_f32_ceil(x: f32) -> f32 {
    x.ceil()
}

/// Implementation of f32.floor
#[unsafe(no_mangle)]
pub extern "C" fn wasmer_vm_f32_floor(x: f32) -> f32 {
    x.floor()
}

/// Implementation of f32.trunc
#[unsafe(no_mangle)]
pub extern "C" fn wasmer_vm_f32_trunc(x: f32) -> f32 {
    x.trunc()
}

/// Implementation of f32.nearest
#[allow(clippy::float_arithmetic, clippy::float_cmp)]
#[unsafe(no_mangle)]
pub extern "C" fn wasmer_vm_f32_nearest(x: f32) -> f32 {
    // Rust doesn't have a nearest function, so do it manually.
    if x == 0.0 {
        // Preserve the sign of zero.
        x
    } else {
        // Nearest is either ceil or floor depending on which is nearest or even.
        let u = x.ceil();
        let d = x.floor();
        let um = (x - u).abs();
        let dm = (x - d).abs();
        if um < dm
            || (um == dm && {
                let h = u / 2.;
                h.floor() == h
            })
        {
            u
        } else {
            d
        }
    }
}

/// Implementation of f32.sqrt
#[unsafe(no_mangle)]
pub extern "C" fn wasmer_vm_f32_sqrt(x: f32) -> f32 {
    x.sqrt()
}

/// Implementation of f64.sqrt
#[unsafe(no_mangle)]
pub extern "C" fn wasmer_vm_f64_sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// Implementation of f64.ceil
#[unsafe(no_mangle)]
pub extern "C" fn wasmer_vm_f64_ceil(x: f64) -> f64 {
    x.ceil()
}

/// Implementation of f64.floor
#[unsafe(no_mangle)]
pub extern "C" fn wasmer_vm_f64_floor(x: f64) -> f64 {
    x.floor()
}

/// Implementation of f64.trunc
#[unsafe(no_mangle)]
pub extern "C" fn wasmer_vm_f64_trunc(x: f64) -> f64 {
    x.trunc()
}

/// Implementation of f64.nearest
#[allow(clippy::float_arithmetic, clippy::float_cmp)]
#[unsafe(no_mangle)]
pub extern "C" fn wasmer_vm_f64_nearest(x: f64) -> f64 {
    // Rust doesn't have a nearest function, so do it manually.
    if x == 0.0 {
        // Preserve the sign of zero.
        x
    } else {
        // Nearest is either ceil or floor depending on which is nearest or even.
        let u = x.ceil();
        let d = x.floor();
        let um = (x - u).abs();
        let dm = (x - d).abs();
        if um < dm
            || (um == dm && {
                let h = u / 2.;
                h.floor() == h
            })
        {
            u
        } else {
            d
        }
    }
}

/// Implementation of memory.grow for locally-defined 32-bit memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_memory32_grow(
    vmctx: *mut VMContext,
    delta_in_pages: u32,
    memory_index: u32,
) -> u32 {
    unsafe {
        on_host_stack(|| {
            let instance = (*vmctx).instance_mut();
            let memory_index = LocalMemoryIndex::from_u32(memory_index);

            instance
                .memory_grow(memory_index, delta_in_pages)
                .map_or(u32::MAX, |pages| pages.0)
        })
    }
}

/// Implementation of memory.grow for imported 32-bit memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_imported_memory32_grow(
    vmctx: *mut VMContext,
    delta_in_pages: u32,
    memory_index: u32,
) -> u32 {
    unsafe {
        on_host_stack(|| {
            let instance = (*vmctx).instance_mut();
            let memory_index = MemoryIndex::from_u32(memory_index);

            instance
                .imported_memory_grow(memory_index, delta_in_pages)
                .map_or(u32::MAX, |pages| pages.0)
        })
    }
}

/// Implementation of memory.size for locally-defined 32-bit memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_memory32_size(vmctx: *mut VMContext, memory_index: u32) -> u32 {
    unsafe {
        let instance = (*vmctx).instance();
        let memory_index = LocalMemoryIndex::from_u32(memory_index);

        instance.memory_size(memory_index).0
    }
}

/// Implementation of memory.size for imported 32-bit memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_imported_memory32_size(
    vmctx: *mut VMContext,
    memory_index: u32,
) -> u32 {
    unsafe {
        let instance = (*vmctx).instance();
        let memory_index = MemoryIndex::from_u32(memory_index);

        instance.imported_memory_size(memory_index).0
    }
}

/// Implementation of `table.copy`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_table_copy(
    vmctx: *mut VMContext,
    dst_table_index: u32,
    src_table_index: u32,
    dst: u32,
    src: u32,
    len: u32,
) {
    unsafe {
        let result = {
            let dst_table_index = TableIndex::from_u32(dst_table_index);
            let src_table_index = TableIndex::from_u32(src_table_index);
            (*vmctx)
                .instance_mut()
                .table_copy(dst_table_index, src_table_index, dst, src, len)
        };
        if let Err(trap) = result {
            raise_lib_trap(trap);
        }
    }
}

/// Implementation of `table.init`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_table_init(
    vmctx: *mut VMContext,
    table_index: u32,
    elem_index: u32,
    dst: u32,
    src: u32,
    len: u32,
) {
    unsafe {
        let result = {
            let table_index = TableIndex::from_u32(table_index);
            let elem_index = ElemIndex::from_u32(elem_index);
            let instance = (*vmctx).instance_mut();
            instance.table_init(table_index, elem_index, dst, src, len)
        };
        if let Err(trap) = result {
            raise_lib_trap(trap);
        }
    }
}

/// Implementation of `table.fill`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_table_fill(
    vmctx: *mut VMContext,
    table_index: u32,
    start_idx: u32,
    item: RawTableElement,
    len: u32,
) {
    unsafe {
        let result = {
            let table_index = TableIndex::from_u32(table_index);
            let instance = (*vmctx).instance_mut();
            let elem = match instance.get_table(table_index).ty().ty {
                Type::ExternRef => TableElement::ExternRef(item.extern_ref),
                Type::FuncRef => TableElement::FuncRef(item.func_ref),
                _ => panic!("Unrecognized table type: does not contain references"),
            };

            instance.table_fill(table_index, start_idx, elem, len)
        };
        if let Err(trap) = result {
            raise_lib_trap(trap);
        }
    }
}

/// Implementation of `table.size`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_table_size(vmctx: *mut VMContext, table_index: u32) -> u32 {
    unsafe {
        let instance = (*vmctx).instance();
        let table_index = LocalTableIndex::from_u32(table_index);

        instance.table_size(table_index)
    }
}

/// Implementation of `table.size` for imported tables.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_imported_table_size(
    vmctx: *mut VMContext,
    table_index: u32,
) -> u32 {
    unsafe {
        let instance = (*vmctx).instance();
        let table_index = TableIndex::from_u32(table_index);

        instance.imported_table_size(table_index)
    }
}

/// Implementation of `table.get`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_table_get(
    vmctx: *mut VMContext,
    table_index: u32,
    elem_index: u32,
) -> RawTableElement {
    unsafe {
        let instance = (*vmctx).instance();
        let table_index = LocalTableIndex::from_u32(table_index);

        // TODO: type checking, maybe have specialized accessors
        match instance.table_get(table_index, elem_index) {
            Some(table_ref) => table_ref.into(),
            None => raise_lib_trap(Trap::lib(TrapCode::TableAccessOutOfBounds)),
        }
    }
}

/// Implementation of `table.get` for imported tables.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_imported_table_get(
    vmctx: *mut VMContext,
    table_index: u32,
    elem_index: u32,
) -> RawTableElement {
    unsafe {
        let instance = (*vmctx).instance_mut();
        let table_index = TableIndex::from_u32(table_index);

        // TODO: type checking, maybe have specialized accessors
        match instance.imported_table_get(table_index, elem_index) {
            Some(table_ref) => table_ref.into(),
            None => raise_lib_trap(Trap::lib(TrapCode::TableAccessOutOfBounds)),
        }
    }
}

/// Implementation of `table.set`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
///
/// It is the caller's responsibility to increment the ref count of any ref counted
/// type before passing it to this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_table_set(
    vmctx: *mut VMContext,
    table_index: u32,
    elem_index: u32,
    value: RawTableElement,
) {
    unsafe {
        let instance = (*vmctx).instance_mut();
        let table_index = TableIndex::from_u32(table_index);
        let table_index = instance
            .module_ref()
            .local_table_index(table_index)
            .unwrap();

        let elem = match instance.get_local_table(table_index).ty().ty {
            Type::ExternRef => TableElement::ExternRef(value.extern_ref),
            Type::FuncRef => TableElement::FuncRef(value.func_ref),
            _ => panic!("Unrecognized table type: does not contain references"),
        };

        // TODO: type checking, maybe have specialized accessors
        let result = instance.table_set(table_index, elem_index, elem);

        if let Err(trap) = result {
            raise_lib_trap(trap);
        }
    }
}

/// Implementation of `table.set` for imported tables.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_imported_table_set(
    vmctx: *mut VMContext,
    table_index: u32,
    elem_index: u32,
    value: RawTableElement,
) {
    unsafe {
        let instance = (*vmctx).instance_mut();
        let table_index = TableIndex::from_u32(table_index);
        let elem = match instance.get_table(table_index).ty().ty {
            Type::ExternRef => TableElement::ExternRef(value.extern_ref),
            Type::FuncRef => TableElement::FuncRef(value.func_ref),
            _ => panic!("Unrecognized table type: does not contain references"),
        };

        let result = instance.imported_table_set(table_index, elem_index, elem);

        if let Err(trap) = result {
            raise_lib_trap(trap);
        }
    }
}

/// Implementation of `table.grow` for locally-defined tables.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_table_grow(
    vmctx: *mut VMContext,
    init_value: RawTableElement,
    delta: u32,
    table_index: u32,
) -> u32 {
    unsafe {
        on_host_stack(|| {
            let instance = (*vmctx).instance_mut();
            let table_index = LocalTableIndex::from_u32(table_index);

            let init_value = match instance.get_local_table(table_index).ty().ty {
                Type::ExternRef => TableElement::ExternRef(init_value.extern_ref),
                Type::FuncRef => TableElement::FuncRef(init_value.func_ref),
                _ => panic!("Unrecognized table type: does not contain references"),
            };

            instance
                .table_grow(table_index, delta, init_value)
                .unwrap_or(u32::MAX)
        })
    }
}

/// Implementation of `table.grow` for imported tables.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_imported_table_grow(
    vmctx: *mut VMContext,
    init_value: RawTableElement,
    delta: u32,
    table_index: u32,
) -> u32 {
    unsafe {
        on_host_stack(|| {
            let instance = (*vmctx).instance_mut();
            let table_index = TableIndex::from_u32(table_index);
            let init_value = match instance.get_table(table_index).ty().ty {
                Type::ExternRef => TableElement::ExternRef(init_value.extern_ref),
                Type::FuncRef => TableElement::FuncRef(init_value.func_ref),
                _ => panic!("Unrecognized table type: does not contain references"),
            };

            instance
                .imported_table_grow(table_index, delta, init_value)
                .unwrap_or(u32::MAX)
        })
    }
}

/// Implementation of `func.ref`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_func_ref(
    vmctx: *mut VMContext,
    function_index: u32,
) -> VMFuncRef {
    unsafe {
        let instance = (*vmctx).instance();
        let function_index = FunctionIndex::from_u32(function_index);

        instance.func_ref(function_index).unwrap()
    }
}

/// Implementation of `elem.drop`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_elem_drop(vmctx: *mut VMContext, elem_index: u32) {
    unsafe {
        on_host_stack(|| {
            let elem_index = ElemIndex::from_u32(elem_index);
            let instance = (*vmctx).instance();
            instance.elem_drop(elem_index);
        })
    }
}

/// Implementation of `memory.copy` for locally defined memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_memory32_copy(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    src: u32,
    len: u32,
) {
    unsafe {
        let result = {
            let memory_index = LocalMemoryIndex::from_u32(memory_index);
            let instance = (*vmctx).instance();
            instance.local_memory_copy(memory_index, dst, src, len)
        };
        if let Err(trap) = result {
            raise_lib_trap(trap);
        }
    }
}

/// Implementation of `memory.copy` for imported memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_imported_memory32_copy(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    src: u32,
    len: u32,
) {
    unsafe {
        let result = {
            let memory_index = MemoryIndex::from_u32(memory_index);
            let instance = (*vmctx).instance();
            instance.imported_memory_copy(memory_index, dst, src, len)
        };
        if let Err(trap) = result {
            raise_lib_trap(trap);
        }
    }
}

/// Implementation of `memory.fill` for locally defined memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_memory32_fill(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    val: u32,
    len: u32,
) {
    unsafe {
        let result = {
            let memory_index = LocalMemoryIndex::from_u32(memory_index);
            let instance = (*vmctx).instance();
            instance.local_memory_fill(memory_index, dst, val, len)
        };
        if let Err(trap) = result {
            raise_lib_trap(trap);
        }
    }
}

/// Implementation of `memory.fill` for imported memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_imported_memory32_fill(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    val: u32,
    len: u32,
) {
    unsafe {
        let result = {
            let memory_index = MemoryIndex::from_u32(memory_index);
            let instance = (*vmctx).instance();
            instance.imported_memory_fill(memory_index, dst, val, len)
        };
        if let Err(trap) = result {
            raise_lib_trap(trap);
        }
    }
}

/// Implementation of `memory.init`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_memory32_init(
    vmctx: *mut VMContext,
    memory_index: u32,
    data_index: u32,
    dst: u32,
    src: u32,
    len: u32,
) {
    unsafe {
        let result = {
            let memory_index = MemoryIndex::from_u32(memory_index);
            let data_index = DataIndex::from_u32(data_index);
            let instance = (*vmctx).instance();
            instance.memory_init(memory_index, data_index, dst, src, len)
        };
        if let Err(trap) = result {
            raise_lib_trap(trap);
        }
    }
}

/// Implementation of `data.drop`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_data_drop(vmctx: *mut VMContext, data_index: u32) {
    unsafe {
        on_host_stack(|| {
            let data_index = DataIndex::from_u32(data_index);
            let instance = (*vmctx).instance();
            instance.data_drop(data_index)
        })
    }
}

/// Implementation for raising a trap
///
/// # Safety
///
/// Only safe to call when wasm code is on the stack, aka `wasmer_call` or
/// `wasmer_call_trampoline` must have been previously called.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_raise_trap(trap_code: TrapCode) -> ! {
    unsafe {
        let trap = Trap::lib(trap_code);
        raise_lib_trap(trap)
    }
}

/// (debug) Print an usize.
#[unsafe(no_mangle)]
pub extern "C-unwind" fn wasmer_vm_dbg_usize(value: usize) {
    #[allow(clippy::print_stdout)]
    {
        println!("wasmer_vm_dbg_usize: {value}");
    }
}

/// (debug) Print a string.
#[unsafe(no_mangle)]
pub extern "C-unwind" fn wasmer_vm_dbg_str(ptr: usize, len: u32) {
    #[allow(clippy::print_stdout)]
    unsafe {
        let str = std::str::from_utf8(std::slice::from_raw_parts(ptr as _, len as _))
            .unwrap_or("wasmer_vm_dbg_str failed");
        eprintln!("{str}");
    }
}

/// Implementation for throwing an exception.
///
/// # Safety
///
/// Calls libunwind to perform unwinding magic.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn wasmer_vm_throw(vmctx: *mut VMContext, exnref: u32) -> ! {
    let instance = unsafe { (*vmctx).instance() };
    unsafe { eh::throw(instance.context(), exnref) }
}

/// Implementation for allocating an exception. Returns the exnref, i.e. a handle to the
/// exception within the store.
///
/// # Safety
///
/// The vmctx pointer must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn wasmer_vm_alloc_exception(vmctx: *mut VMContext, tag: u32) -> u32 {
    let instance = unsafe { (*vmctx).instance_mut() };
    let unique_tag = instance.shared_tag_ptr(TagIndex::from_u32(tag)).index();
    let exn = VMExceptionObj::new_zeroed(
        instance.context(),
        InternalStoreHandle::from_index(unique_tag as usize).unwrap(),
    );
    let exnref = InternalStoreHandle::new(instance.context_mut(), exn);
    exnref.index() as u32
}

/// Given a VMContext and an exnref (handle to an exception within the store),
/// returns a pointer to the payload buffer of the underlying VMExceptionObj.
#[unsafe(no_mangle)]
pub extern "C-unwind" fn wasmer_vm_read_exnref(
    vmctx: *mut VMContext,
    exnref: u32,
) -> *mut RawValue {
    let exn = eh::exn_obj_from_exnref(vmctx, exnref);
    unsafe { (*exn).payload().as_ptr() as *mut RawValue }
}

/// Given a pointer to a caught exception, return the exnref contained within.
///
/// # Safety
///
/// `exception` must be a pointer the platform-specific exception type; this is
/// `UwExceptionWrapper` for gcc.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn wasmer_vm_exception_into_exnref(exception: *mut c_void) -> u32 {
    unsafe {
        let exnref = eh::read_exnref(exception);
        eh::delete_exception(exception);
        exnref
    }
}

/// Probestack check
///
/// # Safety
///
/// This function does not follow the standard function ABI, and is called as
/// part of the function prologue.
#[unsafe(no_mangle)]
pub static WASMER_VM_PROBESTACK: unsafe extern "C" fn() = PROBESTACK;

/// Implementation of memory.wait32 for locally-defined 32-bit memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_memory32_atomic_wait32(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    val: u32,
    timeout: i64,
) -> u32 {
    unsafe {
        let result = {
            let instance = (*vmctx).instance_mut();
            let memory_index = LocalMemoryIndex::from_u32(memory_index);

            instance.local_memory_wait32(memory_index, dst, val, timeout)
        };
        if let Err(trap) = result {
            raise_lib_trap(trap);
        }
        result.unwrap()
    }
}

/// Implementation of memory.wait32 for imported 32-bit memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_imported_memory32_atomic_wait32(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    val: u32,
    timeout: i64,
) -> u32 {
    unsafe {
        let result = {
            let instance = (*vmctx).instance_mut();
            let memory_index = MemoryIndex::from_u32(memory_index);

            instance.imported_memory_wait32(memory_index, dst, val, timeout)
        };
        if let Err(trap) = result {
            raise_lib_trap(trap);
        }
        result.unwrap()
    }
}

/// Implementation of memory.wait64 for locally-defined 32-bit memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_memory32_atomic_wait64(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    val: u64,
    timeout: i64,
) -> u32 {
    unsafe {
        let result = {
            let instance = (*vmctx).instance_mut();
            let memory_index = LocalMemoryIndex::from_u32(memory_index);

            instance.local_memory_wait64(memory_index, dst, val, timeout)
        };
        if let Err(trap) = result {
            raise_lib_trap(trap);
        }
        result.unwrap()
    }
}

/// Implementation of memory.wait64 for imported 32-bit memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_imported_memory32_atomic_wait64(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    val: u64,
    timeout: i64,
) -> u32 {
    unsafe {
        let result = {
            let instance = (*vmctx).instance_mut();
            let memory_index = MemoryIndex::from_u32(memory_index);

            instance.imported_memory_wait64(memory_index, dst, val, timeout)
        };
        if let Err(trap) = result {
            raise_lib_trap(trap);
        }
        result.unwrap()
    }
}

/// Implementation of memory.notify for locally-defined 32-bit memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_memory32_atomic_notify(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    cnt: u32,
) -> u32 {
    unsafe {
        let result = {
            let instance = (*vmctx).instance_mut();
            let memory_index = LocalMemoryIndex::from_u32(memory_index);

            instance.local_memory_notify(memory_index, dst, cnt)
        };
        if let Err(trap) = result {
            raise_lib_trap(trap);
        }
        result.unwrap()
    }
}

/// Implementation of memory.notify for imported 32-bit memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_vm_imported_memory32_atomic_notify(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    cnt: u32,
) -> u32 {
    unsafe {
        let result = {
            let instance = (*vmctx).instance_mut();
            let memory_index = MemoryIndex::from_u32(memory_index);

            instance.imported_memory_notify(memory_index, dst, cnt)
        };
        if let Err(trap) = result {
            raise_lib_trap(trap);
        }
        result.unwrap()
    }
}

/// The function pointer to a libcall
pub fn function_pointer(libcall: LibCall) -> usize {
    match libcall {
        LibCall::CeilF32 => wasmer_vm_f32_ceil as *const () as usize,
        LibCall::CeilF64 => wasmer_vm_f64_ceil as *const () as usize,
        LibCall::FloorF32 => wasmer_vm_f32_floor as *const () as usize,
        LibCall::FloorF64 => wasmer_vm_f64_floor as *const () as usize,
        LibCall::NearestF32 => wasmer_vm_f32_nearest as *const () as usize,
        LibCall::NearestF64 => wasmer_vm_f64_nearest as *const () as usize,
        LibCall::SqrtF32 => wasmer_vm_f32_sqrt as *const () as usize,
        LibCall::SqrtF64 => wasmer_vm_f64_sqrt as *const () as usize,
        LibCall::TruncF32 => wasmer_vm_f32_trunc as *const () as usize,
        LibCall::TruncF64 => wasmer_vm_f64_trunc as *const () as usize,
        LibCall::Memory32Size => wasmer_vm_memory32_size as *const () as usize,
        LibCall::ImportedMemory32Size => wasmer_vm_imported_memory32_size as *const () as usize,
        LibCall::TableCopy => wasmer_vm_table_copy as *const () as usize,
        LibCall::TableInit => wasmer_vm_table_init as *const () as usize,
        LibCall::TableFill => wasmer_vm_table_fill as *const () as usize,
        LibCall::TableSize => wasmer_vm_table_size as *const () as usize,
        LibCall::ImportedTableSize => wasmer_vm_imported_table_size as *const () as usize,
        LibCall::TableGet => wasmer_vm_table_get as *const () as usize,
        LibCall::ImportedTableGet => wasmer_vm_imported_table_get as *const () as usize,
        LibCall::TableSet => wasmer_vm_table_set as *const () as usize,
        LibCall::ImportedTableSet => wasmer_vm_imported_table_set as *const () as usize,
        LibCall::TableGrow => wasmer_vm_table_grow as *const () as usize,
        LibCall::ImportedTableGrow => wasmer_vm_imported_table_grow as *const () as usize,
        LibCall::FuncRef => wasmer_vm_func_ref as *const () as usize,
        LibCall::ElemDrop => wasmer_vm_elem_drop as *const () as usize,
        LibCall::Memory32Copy => wasmer_vm_memory32_copy as *const () as usize,
        LibCall::ImportedMemory32Copy => wasmer_vm_imported_memory32_copy as *const () as usize,
        LibCall::Memory32Fill => wasmer_vm_memory32_fill as *const () as usize,
        LibCall::ImportedMemory32Fill => wasmer_vm_imported_memory32_fill as *const () as usize,
        LibCall::Memory32Init => wasmer_vm_memory32_init as *const () as usize,
        LibCall::DataDrop => wasmer_vm_data_drop as *const () as usize,
        LibCall::Probestack => WASMER_VM_PROBESTACK as *const () as usize,
        LibCall::RaiseTrap => wasmer_vm_raise_trap as *const () as usize,
        LibCall::Memory32AtomicWait32 => wasmer_vm_memory32_atomic_wait32 as *const () as usize,
        LibCall::ImportedMemory32AtomicWait32 => {
            wasmer_vm_imported_memory32_atomic_wait32 as *const () as usize
        }
        LibCall::Memory32AtomicWait64 => wasmer_vm_memory32_atomic_wait64 as *const () as usize,
        LibCall::ImportedMemory32AtomicWait64 => {
            wasmer_vm_imported_memory32_atomic_wait64 as *const () as usize
        }
        LibCall::Memory32AtomicNotify => wasmer_vm_memory32_atomic_notify as *const () as usize,
        LibCall::ImportedMemory32AtomicNotify => {
            wasmer_vm_imported_memory32_atomic_notify as *const () as usize
        }
        LibCall::Throw => wasmer_vm_throw as *const () as usize,
        LibCall::EHPersonality => eh::wasmer_eh_personality as *const () as usize,
        LibCall::EHPersonality2 => eh::wasmer_eh_personality2 as *const () as usize,
        LibCall::AllocException => wasmer_vm_alloc_exception as *const () as usize,
        LibCall::ReadExnRef => wasmer_vm_read_exnref as *const () as usize,
        LibCall::LibunwindExceptionIntoExnRef => {
            wasmer_vm_exception_into_exnref as *const () as usize
        }
        LibCall::DebugUsize => wasmer_vm_dbg_usize as *const () as usize,
        LibCall::DebugStr => wasmer_vm_dbg_str as *const () as usize,
        // --- Soft-float libcalls ---
        // compiler-rt / libgcc provides these on every std Rust target.
        // On wasm32 the JIT engine is never active, so these variants are unreachable.
        _lc @ (LibCall::Addsf3
        | LibCall::Adddf3
        | LibCall::Subsf3
        | LibCall::Subdf3
        | LibCall::Mulsf3
        | LibCall::Muldf3
        | LibCall::Divsf3
        | LibCall::Divdf3
        | LibCall::Negsf2
        | LibCall::Negdf2
        | LibCall::Extendsfdf2
        | LibCall::Truncdfsf2
        | LibCall::Fixsfsi
        | LibCall::Fixdfsi
        | LibCall::Fixsfdi
        | LibCall::Fixdfdi
        | LibCall::Fixunssfsi
        | LibCall::Fixunsdfsi
        | LibCall::Fixunssfdi
        | LibCall::Fixunsdfdi
        | LibCall::Floatsisf
        | LibCall::Floatsidf
        | LibCall::Floatdisf
        | LibCall::Floatdidf
        | LibCall::Floatunsisf
        | LibCall::Floatunsidf
        | LibCall::Floatundisf
        | LibCall::Floatundidf
        | LibCall::Unordsf2
        | LibCall::Unorddf2
        | LibCall::Eqsf2
        | LibCall::Eqdf2
        | LibCall::Nesf2
        | LibCall::Nedf2
        | LibCall::Gesf2
        | LibCall::Gedf2
        | LibCall::Ltsf2
        | LibCall::Ltdf2
        | LibCall::Lesf2
        | LibCall::Ledf2
        | LibCall::Gtsf2
        | LibCall::Gtdf2) => {
            #[cfg(target_arch = "wasm32")]
            unreachable!("soft-float libcalls are not reachable on wasm32");
            #[cfg(not(target_arch = "wasm32"))]
            match _lc {
                LibCall::Addsf3 => __addsf3 as *const () as usize,
                LibCall::Adddf3 => __adddf3 as *const () as usize,
                LibCall::Subsf3 => __subsf3 as *const () as usize,
                LibCall::Subdf3 => __subdf3 as *const () as usize,
                LibCall::Mulsf3 => __mulsf3 as *const () as usize,
                LibCall::Muldf3 => __muldf3 as *const () as usize,
                LibCall::Divsf3 => __divsf3 as *const () as usize,
                LibCall::Divdf3 => __divdf3 as *const () as usize,
                LibCall::Negsf2 => __negsf2 as *const () as usize,
                LibCall::Negdf2 => __negdf2 as *const () as usize,
                LibCall::Extendsfdf2 => __extendsfdf2 as *const () as usize,
                LibCall::Truncdfsf2 => __truncdfsf2 as *const () as usize,
                LibCall::Fixsfsi => __fixsfsi as *const () as usize,
                LibCall::Fixdfsi => __fixdfsi as *const () as usize,
                LibCall::Fixsfdi => __fixsfdi as *const () as usize,
                LibCall::Fixdfdi => __fixdfdi as *const () as usize,
                LibCall::Fixunssfsi => __fixunssfsi as *const () as usize,
                LibCall::Fixunsdfsi => __fixunsdfsi as *const () as usize,
                LibCall::Fixunssfdi => __fixunssfdi as *const () as usize,
                LibCall::Fixunsdfdi => __fixunsdfdi as *const () as usize,
                LibCall::Floatsisf => __floatsisf as *const () as usize,
                LibCall::Floatsidf => __floatsidf as *const () as usize,
                LibCall::Floatdisf => __floatdisf as *const () as usize,
                LibCall::Floatdidf => __floatdidf as *const () as usize,
                LibCall::Floatunsisf => __floatunsisf as *const () as usize,
                LibCall::Floatunsidf => __floatunsidf as *const () as usize,
                LibCall::Floatundisf => __floatundisf as *const () as usize,
                LibCall::Floatundidf => __floatundidf as *const () as usize,
                LibCall::Unordsf2 => __unordsf2 as *const () as usize,
                LibCall::Unorddf2 => __unorddf2 as *const () as usize,
                LibCall::Eqsf2 => __eqsf2 as *const () as usize,
                LibCall::Eqdf2 => __eqdf2 as *const () as usize,
                LibCall::Nesf2 => __nesf2 as *const () as usize,
                LibCall::Nedf2 => __nedf2 as *const () as usize,
                LibCall::Gesf2 => __gesf2 as *const () as usize,
                LibCall::Gedf2 => __gedf2 as *const () as usize,
                LibCall::Ltsf2 => __ltsf2 as *const () as usize,
                LibCall::Ltdf2 => __ltdf2 as *const () as usize,
                LibCall::Lesf2 => __lesf2 as *const () as usize,
                LibCall::Ledf2 => __ledf2 as *const () as usize,
                LibCall::Gtsf2 => __gtsf2 as *const () as usize,
                LibCall::Gtdf2 => __gtdf2 as *const () as usize,
                _ => unreachable!(),
            }
        }
    }
}

// Soft-float arithmetic routines. Provided by compiler-rt / libgcc on all non-wasm targets.
#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" {
    // --- f32/f64 arithmetic ---
    fn __addsf3(a: f32, b: f32) -> f32;
    fn __adddf3(a: f64, b: f64) -> f64;
    fn __subsf3(a: f32, b: f32) -> f32;
    fn __subdf3(a: f64, b: f64) -> f64;
    fn __mulsf3(a: f32, b: f32) -> f32;
    fn __muldf3(a: f64, b: f64) -> f64;
    fn __divsf3(a: f32, b: f32) -> f32;
    fn __divdf3(a: f64, b: f64) -> f64;
    fn __negsf2(a: f32) -> f32;
    fn __negdf2(a: f64) -> f64;
    // --- f32/f64 conversions ---
    fn __extendsfdf2(a: f32) -> f64;
    fn __truncdfsf2(a: f64) -> f32;
    fn __fixsfsi(a: f32) -> i32;
    fn __fixdfsi(a: f64) -> i32;
    fn __fixsfdi(a: f32) -> i64;
    fn __fixdfdi(a: f64) -> i64;
    fn __fixunssfsi(a: f32) -> u32;
    fn __fixunsdfsi(a: f64) -> u32;
    fn __fixunssfdi(a: f32) -> u64;
    fn __fixunsdfdi(a: f64) -> u64;
    fn __floatsisf(i: i32) -> f32;
    fn __floatsidf(i: i32) -> f64;
    fn __floatdisf(i: i64) -> f32;
    fn __floatdidf(i: i64) -> f64;
    fn __floatunsisf(i: u32) -> f32;
    fn __floatunsidf(i: u32) -> f64;
    fn __floatundisf(i: u64) -> f32;
    fn __floatundidf(i: u64) -> f64;
    // --- f32/f64 comparisons (return 0 / nonzero / negative per GCC ABI) ---
    fn __unordsf2(a: f32, b: f32) -> i32;
    fn __unorddf2(a: f64, b: f64) -> i32;
    fn __eqsf2(a: f32, b: f32) -> i32;
    fn __eqdf2(a: f64, b: f64) -> i32;
    fn __nesf2(a: f32, b: f32) -> i32;
    fn __nedf2(a: f64, b: f64) -> i32;
    fn __gesf2(a: f32, b: f32) -> i32;
    fn __gedf2(a: f64, b: f64) -> i32;
    fn __ltsf2(a: f32, b: f32) -> i32;
    fn __ltdf2(a: f64, b: f64) -> i32;
    fn __lesf2(a: f32, b: f32) -> i32;
    fn __ledf2(a: f64, b: f64) -> i32;
    fn __gtsf2(a: f32, b: f32) -> i32;
    fn __gtdf2(a: f64, b: f64) -> i32;
}
