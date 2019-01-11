use crate::{memory::LinearMemory, vm};

pub unsafe extern "C" fn memory_grow_static(
    memory_index: u32,
    by_pages: u32,
    ctx: *mut vm::Ctx,
) -> i32 {
    if let Some(old) = (*(*ctx).local_backing).memories[memory_index as usize].grow_static(by_pages)
    {
        // Store the new size back into the vmctx.
        (*(*ctx).memories.add(memory_index as usize)).size =
            (old as usize + by_pages as usize) * LinearMemory::PAGE_SIZE as usize;
        old
    } else {
        -1
    }
}

pub unsafe extern "C" fn memory_size(memory_index: u32, ctx: *mut vm::Ctx) -> u32 {
    (*(*ctx).local_backing).memories[memory_index as usize].pages()
}

pub unsafe extern "C" fn memory_grow_dynamic(
    memory_index: u32,
    by_pages: u32,
    ctx: *mut vm::Ctx,
) -> i32 {
    if let Some(old) =
        (*(*ctx).local_backing).memories[memory_index as usize].grow_dynamic(by_pages)
    {
        // Store the new size back into the vmctx.
        (*(*ctx).memories.add(memory_index as usize)).size =
            (old as usize + by_pages as usize) * LinearMemory::PAGE_SIZE as usize;
        old
    } else {
        -1
    }
}
