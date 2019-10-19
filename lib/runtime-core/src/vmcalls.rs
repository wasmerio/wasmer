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

pub unsafe extern "C" fn local_static_memory_size(
    ctx: &vm::Ctx,
    memory_index: LocalMemoryIndex,
) -> Pages {
    let local_memory = *ctx.internal.memories.add(memory_index.index());
    let memory = (*local_memory).memory as *mut StaticMemory;

    (*memory).size()
}

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

pub unsafe extern "C" fn local_dynamic_memory_size(
    ctx: &vm::Ctx,
    memory_index: LocalMemoryIndex,
) -> Pages {
    let local_memory = *ctx.internal.memories.add(memory_index.index());
    let memory = (*local_memory).memory as *mut DynamicMemory;

    (*memory).size()
}

// +*****************************+
// |      IMPORTED MEMORIES      |
// +*****************************+

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

pub unsafe extern "C" fn imported_static_memory_size(
    ctx: &vm::Ctx,
    import_memory_index: ImportedMemoryIndex,
) -> Pages {
    let local_memory = *ctx
        .internal
        .imported_memories
        .add(import_memory_index.index());
    let memory = (*local_memory).memory as *mut StaticMemory;

    (*memory).size()
}

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

pub unsafe extern "C" fn imported_dynamic_memory_size(
    ctx: &vm::Ctx,
    memory_index: ImportedMemoryIndex,
) -> Pages {
    let local_memory = *ctx.internal.imported_memories.add(memory_index.index());
    let memory = (*local_memory).memory as *mut DynamicMemory;

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
    unimplemented!()
}

pub unsafe extern "C" fn local_table_size(ctx: &vm::Ctx, table_index: LocalTableIndex) -> u32 {
    let _ = table_index;
    let _ = ctx;
    unimplemented!()
}
