mod function;
mod global;
mod memory;
mod table;

pub use function::*;
pub use global::*;
pub use memory::*;
use std::ptr::NonNull;
use std::sync::Arc;
pub use table::*;
use wasmer::{Extern, Instance};

#[repr(C)]
pub struct wasm_extern_t {
    /// cbindgen:ignore
    // this is how we ensure the instance stays alive
    pub(crate) instance: Option<Arc<Instance>>,
    /// cbindgen:ignore
    pub(crate) inner: Extern,
}

wasm_declare_boxed_vec!(extern);

#[no_mangle]
pub unsafe extern "C" fn wasm_func_as_extern(
    func_ptr: Option<NonNull<wasm_func_t>>,
) -> Option<Box<wasm_extern_t>> {
    let func_ptr = func_ptr?;
    let func = func_ptr.as_ref();

    Some(Box::new(wasm_extern_t {
        instance: func.instance.clone(),
        inner: Extern::Function(func.inner.clone()),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_as_extern(
    global_ptr: Option<NonNull<wasm_global_t>>,
) -> Option<Box<wasm_extern_t>> {
    let global_ptr = global_ptr?;
    let global = global_ptr.as_ref();

    Some(Box::new(wasm_extern_t {
        // update this if global does hold onto an `instance`
        instance: None,
        inner: Extern::Global(global.inner.clone()),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_as_extern(
    memory_ptr: Option<NonNull<wasm_memory_t>>,
) -> Option<Box<wasm_extern_t>> {
    let memory_ptr = memory_ptr?;
    let memory = memory_ptr.as_ref();

    Some(Box::new(wasm_extern_t {
        // update this if global does hold onto an `instance`
        instance: None,
        inner: Extern::Memory(memory.inner.clone()),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_as_extern(
    table_ptr: Option<NonNull<wasm_table_t>>,
) -> Option<Box<wasm_extern_t>> {
    let table_ptr = table_ptr?;
    let table = table_ptr.as_ref();

    Some(Box::new(wasm_extern_t {
        // update this if global does hold onto an `instance`
        instance: None,
        inner: Extern::Table(table.inner.clone()),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_as_func(
    extern_ptr: Option<NonNull<wasm_extern_t>>,
) -> Option<Box<wasm_func_t>> {
    let extern_ptr = extern_ptr?;
    let r#extern = extern_ptr.as_ref();
    if let Extern::Function(f) = &r#extern.inner {
        Some(Box::new(wasm_func_t {
            inner: f.clone(),
            instance: r#extern.instance.clone(),
        }))
    } else {
        None
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_as_global(
    extern_ptr: Option<NonNull<wasm_extern_t>>,
) -> Option<Box<wasm_global_t>> {
    let extern_ptr = extern_ptr?;
    let r#extern = extern_ptr.as_ref();
    if let Extern::Global(g) = &r#extern.inner {
        Some(Box::new(wasm_global_t { inner: g.clone() }))
    } else {
        None
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_as_memory(
    extern_ptr: Option<NonNull<wasm_extern_t>>,
) -> Option<Box<wasm_memory_t>> {
    let extern_ptr = extern_ptr?;
    let r#extern = extern_ptr.as_ref();
    if let Extern::Memory(m) = &r#extern.inner {
        Some(Box::new(wasm_memory_t { inner: m.clone() }))
    } else {
        None
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_as_table(
    extern_ptr: Option<NonNull<wasm_extern_t>>,
) -> Option<Box<wasm_table_t>> {
    let extern_ptr = extern_ptr?;
    let r#extern = extern_ptr.as_ref();
    if let Extern::Table(t) = &r#extern.inner {
        Some(Box::new(wasm_table_t { inner: t.clone() }))
    } else {
        None
    }
}
