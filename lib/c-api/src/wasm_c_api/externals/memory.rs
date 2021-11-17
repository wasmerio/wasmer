use super::super::store::wasm_store_t;
use super::super::types::wasm_memorytype_t;
use super::CApiExternTag;
use std::mem;
use wasmer_api::{Memory, Pages};

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Debug)]
pub struct wasm_memory_t {
    pub(crate) tag: CApiExternTag,
    pub(crate) inner: Box<Memory>,
}

impl wasm_memory_t {
    pub(crate) fn new(memory: Memory) -> Self {
        Self {
            tag: CApiExternTag::Memory,
            inner: Box::new(memory),
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_new(
    store: Option<&wasm_store_t>,
    memory_type: Option<&wasm_memorytype_t>,
) -> Option<Box<wasm_memory_t>> {
    let store = store?;
    let memory_type = memory_type?;

    let memory_type = memory_type.inner().memory_type.clone();
    let memory = c_try!(Memory::new(&store.inner, memory_type));

    Some(Box::new(wasm_memory_t::new(memory)))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_delete(_memory: Option<Box<wasm_memory_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_copy(memory: &wasm_memory_t) -> Box<wasm_memory_t> {
    // do shallow copy
    Box::new(wasm_memory_t::new((&*memory.inner).clone()))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_type(
    memory: Option<&wasm_memory_t>,
) -> Option<Box<wasm_memorytype_t>> {
    let memory = memory?;

    Some(Box::new(wasm_memorytype_t::new(memory.inner.ty().clone())))
}

// get a raw pointer into bytes
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_data(memory: &mut wasm_memory_t) -> *mut u8 {
    mem::transmute::<&[std::cell::Cell<u8>], &[u8]>(&memory.inner.view()[..]) as *const [u8]
        as *const u8 as *mut u8
}

// size in bytes
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_data_size(memory: &wasm_memory_t) -> usize {
    memory.inner.size().bytes().0
}

// size in pages
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_size(memory: &wasm_memory_t) -> u32 {
    memory.inner.size().0 as _
}

// delta is in pages
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_grow(memory: &mut wasm_memory_t, delta: u32) -> bool {
    memory.inner.grow(Pages(delta)).is_ok()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_same(
    wasm_memory1: &wasm_memory_t,
    wasm_memory2: &wasm_memory_t,
) -> bool {
    wasm_memory1.inner.same(&wasm_memory2.inner)
}
