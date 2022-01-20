// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

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
//!   leaking `InstanceHandle`s which leads to never deallocating JIT code,
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

use crate::func_data_registry::VMFuncRef;
use crate::probestack::PROBESTACK;
use crate::table::{RawTableElement, TableElement};
use crate::trap::{raise_lib_trap, Trap, TrapCode};
use crate::vmcontext::VMContext;
use crate::VMExternRef;
use enum_iterator::IntoEnumIterator;
use loupe::MemoryUsage;
#[cfg(feature = "enable-rkyv")]
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use std::fmt;
use wasmer_types::{
    DataIndex, ElemIndex, FunctionIndex, LocalMemoryIndex, LocalTableIndex, MemoryIndex,
    TableIndex, Type,
};

/// Implementation of f32.ceil
#[no_mangle]
pub extern "C" fn wasmer_vm_f32_ceil(x: f32) -> f32 {
    x.ceil()
}

/// Implementation of f32.floor
#[no_mangle]
pub extern "C" fn wasmer_vm_f32_floor(x: f32) -> f32 {
    x.floor()
}

/// Implementation of f32.trunc
#[no_mangle]
pub extern "C" fn wasmer_vm_f32_trunc(x: f32) -> f32 {
    x.trunc()
}

/// Implementation of f32.nearest
#[allow(clippy::float_arithmetic, clippy::float_cmp)]
#[no_mangle]
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

/// Implementation of f64.ceil
#[no_mangle]
pub extern "C" fn wasmer_vm_f64_ceil(x: f64) -> f64 {
    x.ceil()
}

/// Implementation of f64.floor
#[no_mangle]
pub extern "C" fn wasmer_vm_f64_floor(x: f64) -> f64 {
    x.floor()
}

/// Implementation of f64.trunc
#[no_mangle]
pub extern "C" fn wasmer_vm_f64_trunc(x: f64) -> f64 {
    x.trunc()
}

/// Implementation of f64.nearest
#[allow(clippy::float_arithmetic, clippy::float_cmp)]
#[no_mangle]
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
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_memory32_grow(
    vmctx: *mut VMContext,
    delta: u32,
    memory_index: u32,
) -> u32 {
    let instance = (&*vmctx).instance();
    let memory_index = LocalMemoryIndex::from_u32(memory_index);

    instance
        .memory_grow(memory_index, delta)
        .map(|pages| pages.0)
        .unwrap_or(u32::max_value())
}

/// Implementation of memory.grow for imported 32-bit memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_imported_memory32_grow(
    vmctx: *mut VMContext,
    delta: u32,
    memory_index: u32,
) -> u32 {
    let instance = (&*vmctx).instance();
    let memory_index = MemoryIndex::from_u32(memory_index);

    instance
        .imported_memory_grow(memory_index, delta)
        .map(|pages| pages.0)
        .unwrap_or(u32::max_value())
}

/// Implementation of memory.size for locally-defined 32-bit memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_memory32_size(vmctx: *mut VMContext, memory_index: u32) -> u32 {
    let instance = (&*vmctx).instance();
    let memory_index = LocalMemoryIndex::from_u32(memory_index);

    instance.memory_size(memory_index).0
}

/// Implementation of memory.size for imported 32-bit memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_imported_memory32_size(
    vmctx: *mut VMContext,
    memory_index: u32,
) -> u32 {
    let instance = (&*vmctx).instance();
    let memory_index = MemoryIndex::from_u32(memory_index);

    instance.imported_memory_size(memory_index).0
}

/// Implementation of `table.copy`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_table_copy(
    vmctx: *mut VMContext,
    dst_table_index: u32,
    src_table_index: u32,
    dst: u32,
    src: u32,
    len: u32,
) {
    let result = {
        let dst_table_index = TableIndex::from_u32(dst_table_index);
        let src_table_index = TableIndex::from_u32(src_table_index);
        let instance = (&*vmctx).instance();
        let dst_table = instance.get_table(dst_table_index);
        let src_table = instance.get_table(src_table_index);
        dst_table.copy(src_table, dst, src, len)
    };
    if let Err(trap) = result {
        raise_lib_trap(trap);
    }
}

/// Implementation of `table.init`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_table_init(
    vmctx: *mut VMContext,
    table_index: u32,
    elem_index: u32,
    dst: u32,
    src: u32,
    len: u32,
) {
    let result = {
        let table_index = TableIndex::from_u32(table_index);
        let elem_index = ElemIndex::from_u32(elem_index);
        let instance = (&*vmctx).instance();
        instance.table_init(table_index, elem_index, dst, src, len)
    };
    if let Err(trap) = result {
        raise_lib_trap(trap);
    }
}

/// Implementation of `table.fill`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_table_fill(
    vmctx: *mut VMContext,
    table_index: u32,
    start_idx: u32,
    item: RawTableElement,
    len: u32,
) {
    let result = {
        let table_index = TableIndex::from_u32(table_index);
        let instance = (&*vmctx).instance();
        let elem = match instance.get_table(table_index).ty().ty {
            Type::ExternRef => TableElement::ExternRef(item.extern_ref.into()),
            Type::FuncRef => TableElement::FuncRef(item.func_ref),
            _ => panic!("Unrecognized table type: does not contain references"),
        };

        instance.table_fill(table_index, start_idx, elem, len)
    };
    if let Err(trap) = result {
        raise_lib_trap(trap);
    }
}

/// Implementation of `table.size`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_table_size(vmctx: *mut VMContext, table_index: u32) -> u32 {
    let instance = (&*vmctx).instance();
    let table_index = LocalTableIndex::from_u32(table_index);

    instance.table_size(table_index)
}

/// Implementation of `table.size` for imported tables.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_imported_table_size(
    vmctx: *mut VMContext,
    table_index: u32,
) -> u32 {
    let instance = (&*vmctx).instance();
    let table_index = TableIndex::from_u32(table_index);

    instance.imported_table_size(table_index)
}

/// Implementation of `table.get`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_table_get(
    vmctx: *mut VMContext,
    table_index: u32,
    elem_index: u32,
) -> RawTableElement {
    let instance = (&*vmctx).instance();
    let table_index = LocalTableIndex::from_u32(table_index);

    // TODO: type checking, maybe have specialized accessors
    match instance.table_get(table_index, elem_index) {
        Some(table_ref) => table_ref.into(),
        None => raise_lib_trap(Trap::lib(TrapCode::TableAccessOutOfBounds)),
    }
}

/// Implementation of `table.get` for imported tables.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_imported_table_get(
    vmctx: *mut VMContext,
    table_index: u32,
    elem_index: u32,
) -> RawTableElement {
    let instance = (&*vmctx).instance();
    let table_index = TableIndex::from_u32(table_index);

    // TODO: type checking, maybe have specialized accessors
    match instance.imported_table_get(table_index, elem_index) {
        Some(table_ref) => table_ref.into(),
        None => raise_lib_trap(Trap::lib(TrapCode::TableAccessOutOfBounds)),
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
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_table_set(
    vmctx: *mut VMContext,
    table_index: u32,
    elem_index: u32,
    value: RawTableElement,
) {
    let instance = (&*vmctx).instance();
    let table_index = TableIndex::from_u32(table_index);
    let table_index = instance
        .module_ref()
        .local_table_index(table_index)
        .unwrap();

    let elem = match instance.get_local_table(table_index).ty().ty {
        Type::ExternRef => TableElement::ExternRef(value.extern_ref.into()),
        Type::FuncRef => TableElement::FuncRef(value.func_ref),
        _ => panic!("Unrecognized table type: does not contain references"),
    };

    // TODO: type checking, maybe have specialized accessors
    let result = instance.table_set(table_index, elem_index, elem);

    if let Err(trap) = result {
        raise_lib_trap(trap);
    }
}

/// Implementation of `table.set` for imported tables.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_imported_table_set(
    vmctx: *mut VMContext,
    table_index: u32,
    elem_index: u32,
    value: RawTableElement,
) {
    let instance = (&*vmctx).instance();
    let table_index = TableIndex::from_u32(table_index);
    let elem = match instance.get_table(table_index).ty().ty {
        Type::ExternRef => TableElement::ExternRef(value.extern_ref.into()),
        Type::FuncRef => TableElement::FuncRef(value.func_ref),
        _ => panic!("Unrecognized table type: does not contain references"),
    };

    let result = instance.imported_table_set(table_index, elem_index, elem);

    if let Err(trap) = result {
        raise_lib_trap(trap);
    }
}

/// Implementation of `table.grow` for locally-defined tables.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_table_grow(
    vmctx: *mut VMContext,
    init_value: RawTableElement,
    delta: u32,
    table_index: u32,
) -> u32 {
    let instance = (&*vmctx).instance();
    let table_index = LocalTableIndex::from_u32(table_index);

    let init_value = match instance.get_local_table(table_index).ty().ty {
        Type::ExternRef => TableElement::ExternRef(init_value.extern_ref.into()),
        Type::FuncRef => TableElement::FuncRef(init_value.func_ref),
        _ => panic!("Unrecognized table type: does not contain references"),
    };

    instance
        .table_grow(table_index, delta, init_value)
        .unwrap_or(u32::max_value())
}

/// Implementation of `table.grow` for imported tables.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_imported_table_grow(
    vmctx: *mut VMContext,
    init_value: RawTableElement,
    delta: u32,
    table_index: u32,
) -> u32 {
    let instance = (&*vmctx).instance();
    let table_index = TableIndex::from_u32(table_index);
    let init_value = match instance.get_table(table_index).ty().ty {
        Type::ExternRef => TableElement::ExternRef(init_value.extern_ref.into()),
        Type::FuncRef => TableElement::FuncRef(init_value.func_ref),
        _ => panic!("Unrecognized table type: does not contain references"),
    };

    instance
        .imported_table_grow(table_index, delta, init_value)
        .unwrap_or(u32::max_value())
}

/// Implementation of `func.ref`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_func_ref(
    vmctx: *mut VMContext,
    function_index: u32,
) -> VMFuncRef {
    let instance = (&*vmctx).instance();
    let function_index = FunctionIndex::from_u32(function_index);

    instance.func_ref(function_index).unwrap()
}

/// Implementation of externref increment
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
///
/// This function must only be called at precise locations to prevent memory leaks.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_externref_inc(externref: VMExternRef) {
    externref.ref_clone();
}

/// Implementation of externref decrement
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
///
/// This function must only be called at precise locations, otherwise use-after-free
/// and other serious memory bugs may occur.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_externref_dec(mut externref: VMExternRef) {
    externref.ref_drop()
}

/// Implementation of `elem.drop`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_elem_drop(vmctx: *mut VMContext, elem_index: u32) {
    let elem_index = ElemIndex::from_u32(elem_index);
    let instance = (&*vmctx).instance();
    instance.elem_drop(elem_index);
}

/// Implementation of `memory.copy` for locally defined memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_memory32_copy(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    src: u32,
    len: u32,
) {
    let result = {
        let memory_index = LocalMemoryIndex::from_u32(memory_index);
        let instance = (&*vmctx).instance();
        instance.local_memory_copy(memory_index, dst, src, len)
    };
    if let Err(trap) = result {
        raise_lib_trap(trap);
    }
}

/// Implementation of `memory.copy` for imported memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_imported_memory32_copy(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    src: u32,
    len: u32,
) {
    let result = {
        let memory_index = MemoryIndex::from_u32(memory_index);
        let instance = (&*vmctx).instance();
        instance.imported_memory_copy(memory_index, dst, src, len)
    };
    if let Err(trap) = result {
        raise_lib_trap(trap);
    }
}

/// Implementation of `memory.fill` for locally defined memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_memory32_fill(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    val: u32,
    len: u32,
) {
    let result = {
        let memory_index = LocalMemoryIndex::from_u32(memory_index);
        let instance = (&*vmctx).instance();
        instance.local_memory_fill(memory_index, dst, val, len)
    };
    if let Err(trap) = result {
        raise_lib_trap(trap);
    }
}

/// Implementation of `memory.fill` for imported memories.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_imported_memory32_fill(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    val: u32,
    len: u32,
) {
    let result = {
        let memory_index = MemoryIndex::from_u32(memory_index);
        let instance = (&*vmctx).instance();
        instance.imported_memory_fill(memory_index, dst, val, len)
    };
    if let Err(trap) = result {
        raise_lib_trap(trap);
    }
}

/// Implementation of `memory.init`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_memory32_init(
    vmctx: *mut VMContext,
    memory_index: u32,
    data_index: u32,
    dst: u32,
    src: u32,
    len: u32,
) {
    let result = {
        let memory_index = MemoryIndex::from_u32(memory_index);
        let data_index = DataIndex::from_u32(data_index);
        let instance = (&*vmctx).instance();
        instance.memory_init(memory_index, data_index, dst, src, len)
    };
    if let Err(trap) = result {
        raise_lib_trap(trap);
    }
}

/// Implementation of `data.drop`.
///
/// # Safety
///
/// `vmctx` must be dereferenceable.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_data_drop(vmctx: *mut VMContext, data_index: u32) {
    let data_index = DataIndex::from_u32(data_index);
    let instance = (&*vmctx).instance();
    instance.data_drop(data_index)
}

/// Implementation for raising a trap
///
/// # Safety
///
/// Only safe to call when wasm code is on the stack, aka `wasmer_call` or
/// `wasmer_call_trampoline` must have been previously called.
#[no_mangle]
pub unsafe extern "C" fn wasmer_vm_raise_trap(trap_code: TrapCode) -> ! {
    let trap = Trap::lib(trap_code);
    raise_lib_trap(trap)
}

/// Probestack check
///
/// # Safety
///
/// This function does not follow the standard function ABI, and is called as
/// part of the function prologue.
#[no_mangle]
pub static wasmer_vm_probestack: unsafe extern "C" fn() = PROBESTACK;

/// The name of a runtime library routine.
///
/// This list is likely to grow over time.
#[cfg_attr(
    feature = "enable-rkyv",
    derive(RkyvSerialize, RkyvDeserialize, Archive)
)]
#[derive(
    Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, MemoryUsage, IntoEnumIterator,
)]
pub enum LibCall {
    /// ceil.f32
    CeilF32,

    /// ceil.f64
    CeilF64,

    /// floor.f32
    FloorF32,

    /// floor.f64
    FloorF64,

    /// nearest.f32
    NearestF32,

    /// nearest.f64
    NearestF64,

    /// trunc.f32
    TruncF32,

    /// trunc.f64
    TruncF64,

    /// memory.size for local functions
    Memory32Size,

    /// memory.size for imported functions
    ImportedMemory32Size,

    /// table.copy
    TableCopy,

    /// table.init
    TableInit,

    /// table.fill
    TableFill,

    /// table.size for local tables
    TableSize,

    /// table.size for imported tables
    ImportedTableSize,

    /// table.get for local tables
    TableGet,

    /// table.get for imported tables
    ImportedTableGet,

    /// table.set for local tables
    TableSet,

    /// table.set for imported tables
    ImportedTableSet,

    /// table.grow for local tables
    TableGrow,

    /// table.grow for imported tables
    ImportedTableGrow,

    /// ref.func
    FuncRef,

    /// elem.drop
    ElemDrop,

    /// memory.copy for local memories
    Memory32Copy,

    /// memory.copy for imported memories
    ImportedMemory32Copy,

    /// memory.fill for local memories
    Memory32Fill,

    /// memory.fill for imported memories
    ImportedMemory32Fill,

    /// memory.init
    Memory32Init,

    /// data.drop
    DataDrop,

    /// A custom trap
    RaiseTrap,

    /// probe for stack overflow. These are emitted for functions which need
    /// when the `enable_probestack` setting is true.
    Probestack,
}

impl LibCall {
    /// The function pointer to a libcall
    pub fn function_pointer(self) -> usize {
        match self {
            Self::CeilF32 => wasmer_vm_f32_ceil as usize,
            Self::CeilF64 => wasmer_vm_f64_ceil as usize,
            Self::FloorF32 => wasmer_vm_f32_floor as usize,
            Self::FloorF64 => wasmer_vm_f64_floor as usize,
            Self::NearestF32 => wasmer_vm_f32_nearest as usize,
            Self::NearestF64 => wasmer_vm_f64_nearest as usize,
            Self::TruncF32 => wasmer_vm_f32_trunc as usize,
            Self::TruncF64 => wasmer_vm_f64_trunc as usize,
            Self::Memory32Size => wasmer_vm_memory32_size as usize,
            Self::ImportedMemory32Size => wasmer_vm_imported_memory32_size as usize,
            Self::TableCopy => wasmer_vm_table_copy as usize,
            Self::TableInit => wasmer_vm_table_init as usize,
            Self::TableFill => wasmer_vm_table_fill as usize,
            Self::TableSize => wasmer_vm_table_size as usize,
            Self::ImportedTableSize => wasmer_vm_imported_table_size as usize,
            Self::TableGet => wasmer_vm_table_get as usize,
            Self::ImportedTableGet => wasmer_vm_imported_table_get as usize,
            Self::TableSet => wasmer_vm_table_set as usize,
            Self::ImportedTableSet => wasmer_vm_imported_table_set as usize,
            Self::TableGrow => wasmer_vm_table_grow as usize,
            Self::ImportedTableGrow => wasmer_vm_imported_table_grow as usize,
            Self::FuncRef => wasmer_vm_func_ref as usize,
            Self::ElemDrop => wasmer_vm_elem_drop as usize,
            Self::Memory32Copy => wasmer_vm_memory32_copy as usize,
            Self::ImportedMemory32Copy => wasmer_vm_imported_memory32_copy as usize,
            Self::Memory32Fill => wasmer_vm_memory32_fill as usize,
            Self::ImportedMemory32Fill => wasmer_vm_memory32_fill as usize,
            Self::Memory32Init => wasmer_vm_memory32_init as usize,
            Self::DataDrop => wasmer_vm_data_drop as usize,
            Self::Probestack => wasmer_vm_probestack as usize,
            Self::RaiseTrap => wasmer_vm_raise_trap as usize,
        }
    }

    /// Return the function name associated to the libcall.
    pub fn to_function_name(&self) -> &str {
        match self {
            Self::CeilF32 => "wasmer_vm_f32_ceil",
            Self::CeilF64 => "wasmer_vm_f64_ceil",
            Self::FloorF32 => "wasmer_vm_f32_floor",
            Self::FloorF64 => "wasmer_vm_f64_floor",
            Self::NearestF32 => "wasmer_vm_f32_nearest",
            Self::NearestF64 => "wasmer_vm_f64_nearest",
            Self::TruncF32 => "wasmer_vm_f32_trunc",
            Self::TruncF64 => "wasmer_vm_f64_trunc",
            Self::Memory32Size => "wasmer_vm_memory32_size",
            Self::ImportedMemory32Size => "wasmer_vm_imported_memory32_size",
            Self::TableCopy => "wasmer_vm_table_copy",
            Self::TableInit => "wasmer_vm_table_init",
            Self::TableFill => "wasmer_vm_table_fill",
            Self::TableSize => "wasmer_vm_table_size",
            Self::ImportedTableSize => "wasmer_vm_imported_table_size",
            Self::TableGet => "wasmer_vm_table_get",
            Self::ImportedTableGet => "wasmer_vm_imported_table_get",
            Self::TableSet => "wasmer_vm_table_set",
            Self::ImportedTableSet => "wasmer_vm_imported_table_set",
            Self::TableGrow => "wasmer_vm_table_grow",
            Self::ImportedTableGrow => "wasmer_vm_imported_table_grow",
            Self::FuncRef => "wasmer_vm_func_ref",
            Self::ElemDrop => "wasmer_vm_elem_drop",
            Self::Memory32Copy => "wasmer_vm_memory32_copy",
            Self::ImportedMemory32Copy => "wasmer_vm_imported_memory32_copy",
            Self::Memory32Fill => "wasmer_vm_memory32_fill",
            Self::ImportedMemory32Fill => "wasmer_vm_imported_memory32_fill",
            Self::Memory32Init => "wasmer_vm_memory32_init",
            Self::DataDrop => "wasmer_vm_data_drop",
            Self::RaiseTrap => "wasmer_vm_raise_trap",
            // We have to do this because macOS requires a leading `_` and it's not
            // a normal function, it's a static variable, so we have to do it manually.
            #[cfg(target_vendor = "apple")]
            Self::Probestack => "_wasmer_vm_probestack",
            #[cfg(not(target_vendor = "apple"))]
            Self::Probestack => "wasmer_vm_probestack",
        }
    }
}

impl fmt::Display for LibCall {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}
