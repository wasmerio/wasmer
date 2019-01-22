use crate::{
    memory::LinearMemory,
    structures::TypedIndex,
    types::{ImportedMemoryIndex, LocalMemoryIndex, LocalTableIndex},
    vm,
};

// +*****************************+
// |       LOCAL MEMORIES        |
// +****************************+

pub unsafe extern "C" fn local_static_memory_grow(
    memory_index: LocalMemoryIndex,
    by_pages: u32,
    ctx: *mut vm::Ctx,
) -> i32 {
    if let Some(old) = (*(*ctx).local_backing)
        .memory(memory_index)
        .grow_static(by_pages)
    {
        // Store the new size back into the vmctx.
        (*(*ctx).memories.add(memory_index.index())).size =
            (old as usize + by_pages as usize) * LinearMemory::PAGE_SIZE as usize;
        old
    } else {
        -1
    }
}

pub unsafe extern "C" fn local_static_memory_size(
    memory_index: LocalMemoryIndex,
    ctx: *mut vm::Ctx,
) -> u32 {
    (*(*ctx).local_backing).memory(memory_index).pages()
}

pub unsafe extern "C" fn local_dynamic_memory_grow(
    memory_index: LocalMemoryIndex,
    by_pages: u32,
    ctx: *mut vm::Ctx,
) -> i32 {
    if let Some(old) = (*(*ctx).local_backing)
        .memory(memory_index)
        .grow_dynamic(by_pages)
    {
        // Store the new size back into the vmctx.
        (*(*ctx).memories.add(memory_index.index())).size =
            (old as usize + by_pages as usize) * LinearMemory::PAGE_SIZE as usize;
        old
    } else {
        -1
    }
}

// +*****************************+
// |      IMPORTED MEMORIES      |
// +****************************+

pub unsafe extern "C" fn imported_static_memory_grow(
    imported_mem_index: ImportedMemoryIndex,
    by_pages: u32,
    caller_ctx: *mut vm::Ctx,
) -> i32 {
    let import_backing = &*(*caller_ctx).import_backing;
    let vm_imported_mem = import_backing.imported_memory(imported_mem_index);

    // We can assume that the memory here is local to the callee ctx.
    let local_mem_index = (*vm_imported_mem.memory).index;

    if let Some(old) = (*(*vm_imported_mem.vmctx).local_backing)
        .memory(local_mem_index)
        .grow_dynamic(by_pages)
    {
        // Store the new size back into the vmctx.
        (*(*vm_imported_mem.vmctx)
            .memories
            .add(local_mem_index.index()))
        .size = (old as usize + by_pages as usize) * LinearMemory::PAGE_SIZE as usize;
        old
    } else {
        -1
    }
}

pub unsafe extern "C" fn imported_static_memory_size(
    imported_memory_index: ImportedMemoryIndex,
    caller_ctx: *mut vm::Ctx,
) -> u32 {
    let import_backing = &*(*caller_ctx).import_backing;
    let vm_imported_mem = import_backing.imported_memory(imported_memory_index);

    // We can assume that the memory here is local to the callee ctx.
    let local_mem_index = (*vm_imported_mem.memory).index;
    (*(*vm_imported_mem.vmctx).local_backing)
        .memory(local_mem_index)
        .pages()
}

// +*****************************+
// |        LOCAL TABLES         |
// +****************************+

pub unsafe extern "C" fn local_table_grow(
    table_index: LocalTableIndex,
    by_elems: u32,
    ctx: *mut vm::Ctx,
) -> i32 {
    let _ = table_index;
    let _ = by_elems;
    let _ = ctx;
    unimplemented!()
}

pub unsafe extern "C" fn local_table_size(table_index: LocalTableIndex, ctx: *mut vm::Ctx) -> u32 {
    let _ = table_index;
    let _ = ctx;
    unimplemented!()
}
