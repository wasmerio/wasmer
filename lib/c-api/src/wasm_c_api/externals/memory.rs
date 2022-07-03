use super::super::context::wasm_context_t;
use super::super::store::wasm_store_t;
use super::super::types::wasm_memorytype_t;
use super::CApiExternTag;
use std::cell::RefCell;
use std::rc::Rc;
use wasmer_api::{Memory, Pages};

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Debug)]
pub struct wasm_memory_t {
    pub(crate) tag: CApiExternTag,
    pub(crate) inner: Box<Memory>,
    pub(crate) context: Rc<RefCell<wasm_context_t>>,
}

impl wasm_memory_t {
    pub(crate) fn new(memory: Memory, context: Rc<RefCell<wasm_context_t>>) -> Self {
        Self {
            tag: CApiExternTag::Memory,
            inner: Box::new(memory),
            context,
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_new(
    store: Option<&wasm_store_t>,
    memory_type: Option<&wasm_memorytype_t>,
) -> Option<Box<wasm_memory_t>> {
    let memory_type = memory_type?;
    let store = store?;
    let mut ctx = store.context.borrow_mut();

    let memory_type = memory_type.inner().memory_type;
    let memory = c_try!(Memory::new(&mut ctx.inner, memory_type));
    drop(ctx);
    Some(Box::new(wasm_memory_t::new(memory, store.context.clone())))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_delete(_memory: Option<Box<wasm_memory_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_copy(memory: &wasm_memory_t) -> Box<wasm_memory_t> {
    // do shallow copy
    Box::new(wasm_memory_t::new(
        (&*memory.inner).clone(),
        memory.context.clone(),
    ))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_same(
    wasm_memory1: &wasm_memory_t,
    wasm_memory2: &wasm_memory_t,
) -> bool {
    wasm_memory1.inner == wasm_memory2.inner
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_type(
    memory: Option<&wasm_memory_t>,
) -> Option<Box<wasm_memorytype_t>> {
    let memory = memory?;
    let ctx = memory.context.borrow();

    Some(Box::new(wasm_memorytype_t::new(
        memory.inner.ty(&ctx.inner),
    )))
}

// get a raw pointer into bytes
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_data(memory: &mut wasm_memory_t) -> *mut u8 {
    let ctx = memory.context.borrow();
    memory.inner.data_ptr(&ctx.inner)
}

// size in bytes
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_data_size(memory: &wasm_memory_t) -> usize {
    let ctx = memory.context.borrow();
    memory.inner.size(&ctx.inner).bytes().0
}

// size in pages
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_size(memory: &wasm_memory_t) -> u32 {
    let ctx = memory.context.borrow();
    memory.inner.size(&ctx.inner).0 as _
}

// delta is in pages
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_grow(memory: &mut wasm_memory_t, delta: u32) -> bool {
    let mut ctx = memory.context.borrow_mut();
    memory.inner.grow(&mut ctx.inner, Pages(delta)).is_ok()
}
