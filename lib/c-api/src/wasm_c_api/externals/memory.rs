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

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_new(
    store: Option<&mut wasm_store_t>,
    memory_type: Option<&wasm_memorytype_t>,
) -> Option<Box<wasm_memory_t>> {
    let memory_type = memory_type?;
    let store = store?;
    let mut store_mut = store.inner.store_mut();
    let memory_type = memory_type.inner().memory_type;
    let memory = c_try!(Memory::new(&mut store_mut, memory_type));
    Some(Box::new(wasm_memory_t {
        extern_: wasm_extern_t::new(store.inner.clone(), memory.into()),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_delete(_memory: Option<Box<wasm_memory_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_copy(memory: &wasm_memory_t) -> Box<wasm_memory_t> {
    // do shallow copy
    Box::new(memory.clone())
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_same(
    wasm_memory1: &wasm_memory_t,
    wasm_memory2: &wasm_memory_t,
) -> bool {
    wasm_memory1.extern_.memory() == wasm_memory2.extern_.memory()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_type(
    memory: Option<&wasm_memory_t>,
) -> Option<Box<wasm_memorytype_t>> {
    let memory = memory?;
    Some(Box::new(wasm_memorytype_t::new(
        memory.extern_.memory().ty(&memory.extern_.store.store()),
    )))
}

// get a raw pointer into bytes
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_data(memory: &mut wasm_memory_t) -> *mut u8 {
    memory
        .extern_
        .memory()
        .view(&memory.extern_.store.store())
        .data_ptr()
}

// size in bytes
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_data_size(memory: &wasm_memory_t) -> usize {
    memory
        .extern_
        .memory()
        .view(&memory.extern_.store.store())
        .size()
        .bytes()
        .0
}

// size in pages
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_size(memory: &wasm_memory_t) -> u32 {
    memory
        .extern_
        .memory()
        .view(&memory.extern_.store.store())
        .size()
        .0 as _
}

// delta is in pages
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_grow(memory: &mut wasm_memory_t, delta: u32) -> bool {
    memory
        .extern_
        .memory()
        .grow(&mut memory.extern_.store.store_mut(), Pages(delta))
        .is_ok()
}
