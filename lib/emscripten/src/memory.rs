use super::env::get_emscripten_data;
use super::process::abort_with_message;
use crate::EmEnv;
use libc::{c_int, c_void, memcpy, size_t};
// TODO: investigate max pages etc. probably in Wasm Common, maybe reexport
use wasmer::{FunctionEnvMut, Pages, WasmPtr, WASM_MAX_PAGES, WASM_MIN_PAGES, WASM_PAGE_SIZE};

/// emscripten: _emscripten_memcpy_big
pub fn _emscripten_memcpy_big(ctx: FunctionEnvMut<EmEnv>, dest: u32, src: u32, len: u32) -> u32 {
    debug!(
        "emscripten::_emscripten_memcpy_big {}, {}, {}",
        dest, src, len
    );
    let memory = ctx.data().memory(0);
    let dest_addr = emscripten_memory_pointer!(memory.view(&ctx), dest) as *mut c_void;
    let src_addr = emscripten_memory_pointer!(memory.view(&ctx), src) as *mut c_void;
    unsafe {
        memcpy(dest_addr, src_addr, len as size_t);
    }
    dest
}

fn get_heap_size(ctx: &FunctionEnvMut<EmEnv>) -> u32 {
    let memory = ctx.data().memory(0);
    memory.view(&ctx).size().bytes().0 as u32
}

/// emscripten: _emscripten_get_heap_size
pub fn _emscripten_get_heap_size(ctx: FunctionEnvMut<EmEnv>) -> u32 {
    trace!("emscripten::_emscripten_get_heap_size");
    let result = get_heap_size(&ctx);
    trace!("=> {}", result);

    result
}

// From emscripten implementation
fn align_up(mut val: usize, multiple: usize) -> usize {
    if val % multiple > 0 {
        val += multiple - val % multiple;
    }
    val
}

fn resize_heap(ctx: &mut FunctionEnvMut<EmEnv>, requested_size: u32) -> u32 {
    debug!("emscripten::_emscripten_resize_heap {}", requested_size);
    let memory = ctx.data().memory(0);
    let current_memory_pages = memory.view(&ctx).size();
    let current_memory = current_memory_pages.bytes().0 as u32;

    // implementation from emscripten
    let mut new_size = usize::max(
        current_memory as usize,
        WASM_MIN_PAGES as usize * WASM_PAGE_SIZE,
    );
    while new_size < requested_size as usize {
        if new_size <= 0x2000_0000 {
            new_size = align_up(new_size * 2, WASM_PAGE_SIZE);
        } else {
            new_size = usize::min(
                align_up((3 * new_size + 0x8000_0000) / 4, WASM_PAGE_SIZE),
                WASM_PAGE_SIZE * WASM_MAX_PAGES as usize,
            );
        }
    }

    let amount_to_grow = (new_size - current_memory as usize) / WASM_PAGE_SIZE;
    if let Ok(_pages_allocated) = ctx
        .data()
        .memory(0)
        .grow(&mut ctx.as_mut(), Pages(amount_to_grow as u32))
    {
        debug!("{} pages allocated", _pages_allocated.0);
        1
    } else {
        0
    }
}

/// emscripten: _emscripten_resize_heap
/// Note: this function only allows growing the size of heap
pub fn _emscripten_resize_heap(mut ctx: FunctionEnvMut<EmEnv>, requested_size: u32) -> u32 {
    resize_heap(&mut ctx, requested_size)
}

/// emscripten: sbrk
pub fn sbrk(mut ctx: FunctionEnvMut<EmEnv>, increment: i32) -> i32 {
    debug!("emscripten::sbrk");
    // let old_dynamic_top = 0;
    // let new_dynamic_top = 0;
    let top_ptr = get_emscripten_data(&ctx)
        .as_ref()
        .unwrap()
        .globals
        .dynamictop_ptr;

    let dynamictop_ptr = WasmPtr::<i32>::new(top_ptr);
    let old_dynamic_top = {
        let memory = ctx.data().memory(0);
        let memory = memory.view(&ctx);
        dynamictop_ptr.deref(&memory).read().unwrap()
    };
    let new_dynamic_top: i32 = old_dynamic_top + increment;
    let total_memory = get_heap_size(&ctx) as i32;
    debug!(
        " => PTR {}, old: {}, new: {}, increment: {}, total: {}",
        dynamictop_ptr.offset(),
        old_dynamic_top,
        new_dynamic_top,
        increment,
        total_memory
    );
    if increment > 0 && new_dynamic_top < old_dynamic_top || new_dynamic_top < 0 {
        abort_on_cannot_grow_memory_old(ctx);
        return -1;
    }
    if new_dynamic_top > total_memory {
        let resized = resize_heap(&mut ctx, new_dynamic_top as u32);
        if resized == 0 {
            return -1;
        }
    }
    // re-borrow the top ptr
    let memory = ctx.data().memory(0);
    let memory = memory.view(&ctx);
    let dynamictop_ptr = WasmPtr::<i32>::new(top_ptr).deref(&memory);
    dynamictop_ptr.write(new_dynamic_top).unwrap();
    old_dynamic_top as _
}

/// emscripten: getTotalMemory
pub fn get_total_memory(ctx: FunctionEnvMut<EmEnv>) -> u32 {
    debug!("emscripten::get_total_memory");
    // instance.memories[0].current_pages()
    // TODO: Fix implementation
    let memory = ctx.data().memory(0);
    memory.view(&ctx).size().bytes().0 as u32
}

/// emscripten: enlargeMemory
pub fn enlarge_memory(_ctx: FunctionEnvMut<EmEnv>) -> u32 {
    debug!("emscripten::enlarge_memory");
    // instance.memories[0].grow(100);
    // TODO: Fix implementation
    0
}

/// emscripten: abortOnCannotGrowMemory
pub fn abort_on_cannot_grow_memory(ctx: FunctionEnvMut<EmEnv>, _requested_size: u32) -> u32 {
    debug!(
        "emscripten::abort_on_cannot_grow_memory {}",
        _requested_size
    );
    abort_with_message(ctx, "Cannot enlarge memory arrays!");
    0
}

/// emscripten: abortOnCannotGrowMemory
pub fn abort_on_cannot_grow_memory_old(ctx: FunctionEnvMut<EmEnv>) -> u32 {
    debug!("emscripten::abort_on_cannot_grow_memory");
    abort_with_message(ctx, "Cannot enlarge memory arrays!");
    0
}

/// emscripten: segfault
pub fn segfault(ctx: FunctionEnvMut<EmEnv>) {
    debug!("emscripten::segfault");
    abort_with_message(ctx, "segmentation fault");
}

/// emscripten: alignfault
pub fn alignfault(ctx: FunctionEnvMut<EmEnv>) {
    debug!("emscripten::alignfault");
    abort_with_message(ctx, "alignment fault");
}

/// emscripten: ftfault
pub fn ftfault(ctx: FunctionEnvMut<EmEnv>) {
    debug!("emscripten::ftfault");
    abort_with_message(ctx, "Function table mask error");
}

/// emscripten: ___map_file
pub fn ___map_file(_ctx: FunctionEnvMut<EmEnv>, _one: u32, _two: u32) -> c_int {
    debug!("emscripten::___map_file");
    // NOTE: TODO: Em returns -1 here as well. May need to implement properly
    -1
}
