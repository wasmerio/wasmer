use super::externals::{wasm_extern_t, wasm_extern_vec_t};
use super::module::wasm_module_t;
use super::store::wasm_store_t;
use super::trap::wasm_trap_t;
use crate::ordered_resolver::OrderedResolver;
use std::mem;
use std::sync::Arc;
use wasmer::{Extern, Instance};

#[allow(non_camel_case_types)]
pub struct wasm_instance_t {
    pub(crate) inner: Arc<Instance>,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_new(
    _store: &wasm_store_t,
    module: &wasm_module_t,
    imports: &wasm_extern_vec_t,
    // own
    _traps: *mut *mut wasm_trap_t,
) -> Option<Box<wasm_instance_t>> {
    let wasm_module = &module.inner;
    let module_imports = wasm_module.imports();
    let module_import_count = module_imports.len();
    let resolver: OrderedResolver = imports
        .into_slice()
        .map(|imports| imports.iter())
        .unwrap_or_else(|| [].iter())
        .map(|imp| &imp.inner)
        .take(module_import_count)
        .cloned()
        .collect();

    let instance = Arc::new(c_try!(Instance::new(wasm_module, &resolver)));
    Some(Box::new(wasm_instance_t { inner: instance }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_delete(_instance: Option<Box<wasm_instance_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_exports(
    instance: &wasm_instance_t,
    // TODO: review types on wasm_declare_vec, handle the optional pointer part properly
    out: &mut wasm_extern_vec_t,
) {
    let instance = &instance.inner;
    let mut extern_vec = instance
        .exports
        .iter()
        .map(|(name, r#extern)| {
            let function = if let Extern::Function { .. } = r#extern {
                instance.exports.get_function(&name).ok().cloned()
            } else {
                None
            };
            Box::into_raw(Box::new(wasm_extern_t {
                instance: Some(Arc::clone(instance)),
                inner: r#extern.clone(),
            }))
        })
        .collect::<Vec<*mut wasm_extern_t>>();
    extern_vec.shrink_to_fit();

    out.size = extern_vec.len();
    out.data = extern_vec.as_mut_ptr();
    // TODO: double check that the destructor will work correctly here
    mem::forget(extern_vec);
}
