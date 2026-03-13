use super::super::types::wasm_memorytype_t;
use super::{super::store::wasm_store_t, wasm_extern_t};
use wasmer_api::{Extern, Memory, Pages};

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone)]
pub struct wasm_memory_t {
    pub(crate) extern_: wasm_extern_t,
}

impl wasm_memory_t {
    pub(crate) fn try_from(e: &wasm_extern_t) -> Option<&wasm_memory_t> {
        match &e.inner {
            Extern::Memory(_) => Some(unsafe { &*(e as *const _ as *const _) }),
            _ => None,
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_memory_new(
    store: Option<&mut wasm_store_t>,
    memory_type: Option<&wasm_memorytype_t>,
) -> Option<Box<wasm_memory_t>> {
    let memory_type = memory_type?;
    let store = store?;
    let mut store_mut = unsafe { store.inner.store_mut() };
    let memory_type = memory_type.inner().memory_type;
    let memory = c_try!(Memory::new(&mut store_mut, memory_type));
    Some(Box::new(wasm_memory_t {
        extern_: wasm_extern_t::new(store.inner.clone(), memory.into()),
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_memory_delete(_memory: Option<Box<wasm_memory_t>>) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_memory_copy(memory: &wasm_memory_t) -> Box<wasm_memory_t> {
    // do shallow copy
    Box::new(memory.clone())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_memory_same(
    wasm_memory1: &wasm_memory_t,
    wasm_memory2: &wasm_memory_t,
) -> bool {
    wasm_memory1.extern_.memory() == wasm_memory2.extern_.memory()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_memory_type(
    memory: Option<&wasm_memory_t>,
) -> Option<Box<wasm_memorytype_t>> {
    let memory = memory?;
    let store_ref = unsafe { memory.extern_.store.store() };
    Some(Box::new(wasm_memorytype_t::new(
        memory.extern_.memory().ty(&store_ref),
    )))
}

// get a raw pointer into bytes
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_memory_data(memory: Option<&mut wasm_memory_t>) -> *mut u8 {
    let Some(memory) = memory else {
        return std::ptr::null_mut();
    };
    let store_ref = unsafe { memory.extern_.store.store() };
    memory.extern_.memory().view(&store_ref).data_ptr()
}

// size in bytes
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_memory_data_size(memory: Option<&wasm_memory_t>) -> usize {
    let Some(memory) = memory else { return 0 };
    let store_ref = unsafe { memory.extern_.store.store() };
    memory.extern_.memory().view(&store_ref).size().bytes().0
}

// size in pages
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_memory_size(memory: Option<&wasm_memory_t>) -> u32 {
    let Some(memory) = memory else { return 0 };
    let store_ref = unsafe { memory.extern_.store.store() };
    memory.extern_.memory().view(&store_ref).size().0 as _
}

// delta is in pages
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_memory_grow(memory: Option<&mut wasm_memory_t>, delta: u32) -> bool {
    let Some(memory) = memory else { return false };
    let wasm_memory = memory.extern_.memory();
    let mut store_mut = unsafe { memory.extern_.store.store_mut() };
    wasm_memory.grow(&mut store_mut, Pages(delta)).is_ok()
}
