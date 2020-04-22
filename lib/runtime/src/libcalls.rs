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

use crate::probestack::PROBESTACK;
use crate::table::Table;
use crate::trap::raise_lib_trap;
use crate::vmcontext::VMContext;
use serde::{Deserialize, Serialize};
use std::fmt;
use wasm_common::{DataIndex, ElemIndex, LocalMemoryIndex, MemoryIndex, TableIndex};

/// Implementation of f32.ceil
pub extern "C" fn wasmer_f32_ceil(x: f32) -> f32 {
    x.ceil()
}

/// Implementation of f32.floor
pub extern "C" fn wasmer_f32_floor(x: f32) -> f32 {
    x.floor()
}

/// Implementation of f32.trunc
pub extern "C" fn wasmer_f32_trunc(x: f32) -> f32 {
    x.trunc()
}

/// Implementation of f32.nearest
#[allow(clippy::float_arithmetic, clippy::float_cmp)]
pub extern "C" fn wasmer_f32_nearest(x: f32) -> f32 {
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
pub extern "C" fn wasmer_f64_ceil(x: f64) -> f64 {
    x.ceil()
}

/// Implementation of f64.floor
pub extern "C" fn wasmer_f64_floor(x: f64) -> f64 {
    x.floor()
}

/// Implementation of f64.trunc
pub extern "C" fn wasmer_f64_trunc(x: f64) -> f64 {
    x.trunc()
}

/// Implementation of f64.nearest
#[allow(clippy::float_arithmetic, clippy::float_cmp)]
pub extern "C" fn wasmer_f64_nearest(x: f64) -> f64 {
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
pub unsafe extern "C" fn wasmer_memory32_grow(
    vmctx: *mut VMContext,
    delta: u32,
    memory_index: u32,
) -> u32 {
    let instance = (&mut *vmctx).instance();
    let memory_index = LocalMemoryIndex::from_u32(memory_index);

    instance
        .memory_grow(memory_index, delta)
        .map(|pages| pages.0)
        .unwrap_or(u32::max_value())
}

/// Implementation of memory.grow for imported 32-bit memories.
pub unsafe extern "C" fn wasmer_imported_memory32_grow(
    vmctx: *mut VMContext,
    delta: u32,
    memory_index: u32,
) -> u32 {
    let instance = (&mut *vmctx).instance();
    let memory_index = MemoryIndex::from_u32(memory_index);

    instance
        .imported_memory_grow(memory_index, delta)
        .map(|pages| pages.0)
        .unwrap_or(u32::max_value())
}

/// Implementation of memory.size for locally-defined 32-bit memories.
pub unsafe extern "C" fn wasmer_memory32_size(vmctx: *mut VMContext, memory_index: u32) -> u32 {
    let instance = (&mut *vmctx).instance();
    let memory_index = LocalMemoryIndex::from_u32(memory_index);

    instance.memory_size(memory_index).0
}

/// Implementation of memory.size for imported 32-bit memories.
pub unsafe extern "C" fn wasmer_imported_memory32_size(
    vmctx: *mut VMContext,
    memory_index: u32,
) -> u32 {
    let instance = (&mut *vmctx).instance();
    let memory_index = MemoryIndex::from_u32(memory_index);

    instance.imported_memory_size(memory_index).0
}

/// Implementation of `table.copy`.
pub unsafe extern "C" fn wasmer_table_copy(
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
        let instance = (&mut *vmctx).instance();
        let dst_table = instance.get_table(dst_table_index);
        let src_table = instance.get_table(src_table_index);
        Table::copy(dst_table, src_table, dst, src, len)
    };
    if let Err(trap) = result {
        raise_lib_trap(trap);
    }
}

/// Implementation of `table.init`.
pub unsafe extern "C" fn wasmer_table_init(
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
        let instance = (&mut *vmctx).instance();
        instance.table_init(table_index, elem_index, dst, src, len)
    };
    if let Err(trap) = result {
        raise_lib_trap(trap);
    }
}

/// Implementation of `elem.drop`.
pub unsafe extern "C" fn wasmer_elem_drop(vmctx: *mut VMContext, elem_index: u32) {
    let elem_index = ElemIndex::from_u32(elem_index);
    let instance = (&mut *vmctx).instance();
    instance.elem_drop(elem_index);
}

/// Implementation of `memory.copy` for locally defined memories.
pub unsafe extern "C" fn wasmer_local_memory_copy(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    src: u32,
    len: u32,
) {
    let result = {
        let memory_index = LocalMemoryIndex::from_u32(memory_index);
        let instance = (&mut *vmctx).instance();
        instance.local_memory_copy(memory_index, dst, src, len)
    };
    if let Err(trap) = result {
        raise_lib_trap(trap);
    }
}

/// Implementation of `memory.copy` for imported memories.
pub unsafe extern "C" fn wasmer_imported_memory_copy(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    src: u32,
    len: u32,
) {
    let result = {
        let memory_index = MemoryIndex::from_u32(memory_index);
        let instance = (&mut *vmctx).instance();
        instance.imported_memory_copy(memory_index, dst, src, len)
    };
    if let Err(trap) = result {
        raise_lib_trap(trap);
    }
}

/// Implementation of `memory.fill` for locally defined memories.
pub unsafe extern "C" fn wasmer_memory_fill(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    val: u32,
    len: u32,
) {
    let result = {
        let memory_index = LocalMemoryIndex::from_u32(memory_index);
        let instance = (&mut *vmctx).instance();
        instance.local_memory_fill(memory_index, dst, val, len)
    };
    if let Err(trap) = result {
        raise_lib_trap(trap);
    }
}

/// Implementation of `memory.fill` for imported memories.
pub unsafe extern "C" fn wasmer_imported_memory_fill(
    vmctx: *mut VMContext,
    memory_index: u32,
    dst: u32,
    val: u32,
    len: u32,
) {
    let result = {
        let memory_index = MemoryIndex::from_u32(memory_index);
        let instance = (&mut *vmctx).instance();
        instance.imported_memory_fill(memory_index, dst, val, len)
    };
    if let Err(trap) = result {
        raise_lib_trap(trap);
    }
}

/// Implementation of `memory.init`.
pub unsafe extern "C" fn wasmer_memory_init(
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
        let instance = (&mut *vmctx).instance();
        instance.memory_init(memory_index, data_index, dst, src, len)
    };
    if let Err(trap) = result {
        raise_lib_trap(trap);
    }
}

/// Implementation of `data.drop`.
pub unsafe extern "C" fn wasmer_data_drop(vmctx: *mut VMContext, data_index: u32) {
    let data_index = DataIndex::from_u32(data_index);
    let instance = (&mut *vmctx).instance();
    instance.data_drop(data_index)
}

/// The name of a runtime library routine.
///
/// Runtime library calls are generated for Cranelift IR instructions that don't have an equivalent
/// ISA instruction or an easy macro expansion. A `LibCall` is used as a well-known name to refer to
/// the runtime library routine. This way, Cranelift doesn't have to know about the naming
/// convention in the embedding VM's runtime library.
///
/// This list is likely to grow over time.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LibCall {
    /// probe for stack overflow. These are emitted for functions which need
    /// when the `enable_probestack` setting is true.
    Probestack,
    /// ceil.f32
    CeilF32,
    /// ceil.f64
    CeilF64,
    /// floor.f32
    FloorF32,
    /// floor.f64
    FloorF64,
    /// trunc.f32
    TruncF32,
    /// frunc.f64
    TruncF64,
    /// nearest.f32
    NearestF32,
    /// nearest.f64
    NearestF64,
    // /// libc.memcpy
    // Memcpy,
    // /// libc.memset
    // Memset,
    // /// libc.memmove
    // Memmove,

    // /// Elf __tls_get_addr
    // ElfTlsGetAddr,
}

impl LibCall {
    /// The function pointer to a libcall
    pub fn function_pointer(&self) -> usize {
        match &self {
            Self::CeilF32 => wasmer_f32_ceil as usize,
            Self::FloorF32 => wasmer_f32_floor as usize,
            Self::TruncF32 => wasmer_f32_trunc as usize,
            Self::NearestF32 => wasmer_f32_nearest as usize,
            Self::CeilF64 => wasmer_f64_ceil as usize,
            Self::FloorF64 => wasmer_f64_floor as usize,
            Self::TruncF64 => wasmer_f64_trunc as usize,
            Self::NearestF64 => wasmer_f64_nearest as usize,
            Self::Probestack => PROBESTACK as usize,
            // other => panic!("unexpected libcall: {}", other),
        }
    }
}

impl fmt::Display for LibCall {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}
