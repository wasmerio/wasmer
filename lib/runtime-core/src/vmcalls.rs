//! Functions called from the generated code.

#![allow(clippy::cast_ptr_alignment)]

use crate::{
    memory::{DynamicMemory, StaticMemory},
    structures::TypedIndex,
    types::{ImportedMemoryIndex, LocalMemoryIndex, LocalTableIndex},
    units::Pages,
    vm,
};

// +*****************************+
// |       LOCAL MEMORIES        |
// +*****************************+

/// Increase the size of the static local memory with offset `memory_index` by
/// `delta` [`Pages`].
///
/// This function returns the number of pages before growing if successful, or
/// `-1` if the grow failed.
///
/// # Safety
///
/// The offset given by `memory_index` is not bounds-checked or typed-checked.
/// Thus, the offset should be in-bounds and point to a [`StaticMemory`].
pub unsafe extern "C" fn local_static_memory_grow(
    ctx: &mut vm::Ctx,
    memory_index: LocalMemoryIndex,
    delta: Pages,
) -> i32 {
    let local_memory = *ctx.internal.memories.add(memory_index.index());
    let memory = (*local_memory).memory as *mut StaticMemory;

    let ret = match (*memory).grow(delta, &mut *local_memory) {
        Ok(old) => old.0 as i32,
        Err(_) => -1,
    };

    ctx.internal.memory_base = (*local_memory).base;
    ctx.internal.memory_bound = (*local_memory).bound;

    ret
}

/// Get the size of a local [`StaticMemory`] in [`Pages`].
///
/// # Safety
///
/// The offset given by `memory_index` is not bounds-checked or typed-checked.
/// Thus, the offset should be in-bounds and point to a [`StaticMemory`].
pub unsafe extern "C" fn local_static_memory_size(
    ctx: &vm::Ctx,
    memory_index: LocalMemoryIndex,
) -> Pages {
    let local_memory = *ctx.internal.memories.add(memory_index.index());
    let memory = (*local_memory).memory as *const StaticMemory;

    (*memory).size()
}

/// Increase the size of the dynamic local memory with offset `memory_index` by
/// `delta` [`Pages`].
///
/// This function returns the number of pages before growing if successful, or
/// `-1` if the grow failed.
///
/// # Safety
///
/// The offset given by `memory_index` is not bounds-checked or typed-checked.
/// Thus, the offset should be in-bounds and point to a [`DynamicMemory`].
pub unsafe extern "C" fn local_dynamic_memory_grow(
    ctx: &mut vm::Ctx,
    memory_index: LocalMemoryIndex,
    delta: Pages,
) -> i32 {
    let local_memory = *ctx.internal.memories.add(memory_index.index());
    let memory = (*local_memory).memory as *mut DynamicMemory;

    let ret = match (*memory).grow(delta, &mut *local_memory) {
        Ok(old) => old.0 as i32,
        Err(_) => -1,
    };

    ctx.internal.memory_base = (*local_memory).base;
    ctx.internal.memory_bound = (*local_memory).bound;

    ret
}

/// Get the size of a local [`DynamicMemory`] in [`Pages`].
///
/// # Safety
///
/// The offset given by `memory_index` is not bounds-checked or typed-checked.
/// Thus, the offset should be in-bounds and point to a [`DynamicMemory`].
pub unsafe extern "C" fn local_dynamic_memory_size(
    ctx: &vm::Ctx,
    memory_index: LocalMemoryIndex,
) -> Pages {
    let local_memory = *ctx.internal.memories.add(memory_index.index());
    let memory = (*local_memory).memory as *const DynamicMemory;

    (*memory).size()
}

// +*****************************+
// |      IMPORTED MEMORIES      |
// +*****************************+

/// Increase the size of the static imported memory with offset `import_memory_index` by
/// `delta` [`Pages`].
///
/// This function returns the number of pages before growing if successful, or
/// `-1` if the grow failed.
///
/// # Safety
///
/// The offset given by `import_memory_index` is not bounds-checked or typed-checked.
/// Thus, the offset should be in-bounds and point to a [`StaticMemory`].
pub unsafe extern "C" fn imported_static_memory_grow(
    ctx: &mut vm::Ctx,
    import_memory_index: ImportedMemoryIndex,
    delta: Pages,
) -> i32 {
    let local_memory = *ctx
        .internal
        .imported_memories
        .add(import_memory_index.index());
    let memory = (*local_memory).memory as *mut StaticMemory;

    let ret = match (*memory).grow(delta, &mut *local_memory) {
        Ok(old) => old.0 as i32,
        Err(_) => -1,
    };

    ctx.internal.memory_base = (*local_memory).base;
    ctx.internal.memory_bound = (*local_memory).bound;

    ret
}

/// Get the size of an imported [`StaticMemory`] in [`Pages`].
///
/// # Safety
///
/// The offset given by `import_memory_index` is not bounds-checked or typed-checked.
/// Thus, the offset should be in-bounds and point to a [`StaticMemory`].
pub unsafe extern "C" fn imported_static_memory_size(
    ctx: &vm::Ctx,
    import_memory_index: ImportedMemoryIndex,
) -> Pages {
    let local_memory = *ctx
        .internal
        .imported_memories
        .add(import_memory_index.index());
    let memory = (*local_memory).memory as *const StaticMemory;

    (*memory).size()
}

/// Increase the size of the dynamic imported memory with offset `memory_index` by
/// `delta` [`Pages`].
///
/// This function returns the number of pages before growing if successful, or
/// `-1` if the grow failed.
///
/// # Safety
///
/// The offset given by `memory_index` is not bounds-checked or typed-checked.
/// Thus, the offset should be in-bounds and point to a [`DynamicMemory`].
pub unsafe extern "C" fn imported_dynamic_memory_grow(
    ctx: &mut vm::Ctx,
    memory_index: ImportedMemoryIndex,
    delta: Pages,
) -> i32 {
    let local_memory = *ctx.internal.imported_memories.add(memory_index.index());
    let memory = (*local_memory).memory as *mut DynamicMemory;

    let ret = match (*memory).grow(delta, &mut *local_memory) {
        Ok(old) => old.0 as i32,
        Err(_) => -1,
    };

    ctx.internal.memory_base = (*local_memory).base;
    ctx.internal.memory_bound = (*local_memory).bound;

    ret
}

/// Get the size of an imported [`DynamicMemory`] in [`Pages`].
///
/// # Safety
///
/// The offset given by `memory_index` is not bounds-checked or typed-checked.
/// Thus, the offset should be in-bounds and point to a [`DynamicMemory`].
pub unsafe extern "C" fn imported_dynamic_memory_size(
    ctx: &vm::Ctx,
    memory_index: ImportedMemoryIndex,
) -> Pages {
    let local_memory = *ctx.internal.imported_memories.add(memory_index.index());
    let memory = (*local_memory).memory as *const DynamicMemory;

    (*memory).size()
}

// +*****************************+
// |        LOCAL TABLES         |
// +*****************************+

pub unsafe extern "C" fn local_table_grow(
    ctx: &mut vm::Ctx,
    table_index: LocalTableIndex,
    delta: u32,
) -> i32 {
    let _ = table_index;
    let _ = delta;
    let _ = ctx;
    unimplemented!("vmcalls::local_table_grow")
}

pub unsafe extern "C" fn local_table_size(ctx: &vm::Ctx, table_index: LocalTableIndex) -> u32 {
    let _ = table_index;
    let _ = ctx;
    unimplemented!("vmcalls::local_table_size")
}
