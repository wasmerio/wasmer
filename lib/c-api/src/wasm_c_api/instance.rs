use super::externals::{wasm_extern_t, wasm_extern_vec_t};
use super::module::wasm_module_t;
use super::store::wasm_store_t;
use super::trap::wasm_trap_t;
use crate::ordered_resolver::OrderedResolver;
use std::mem;
use std::sync::Arc;
use wasmer::{Extern, Instance, InstantiationError};

#[allow(non_camel_case_types)]
pub struct wasm_instance_t {
    pub(crate) inner: Arc<Instance>,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_new(
    _store: Option<&wasm_store_t>,
    module: Option<&wasm_module_t>,
    imports: Option<&wasm_extern_vec_t>,
    traps: *mut *mut wasm_trap_t,
) -> Option<Box<wasm_instance_t>> {
    let module = module?;
    let imports = imports?;

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

    let instance = match Instance::new(wasm_module, &resolver) {
        Ok(instance) => Arc::new(instance),

        Err(InstantiationError::Link(link_error)) => {
            crate::error::update_last_error(link_error);

            return None;
        }

        Err(InstantiationError::Start(runtime_error)) => {
            let trap: Box<wasm_trap_t> = Box::new(runtime_error.into());
            *traps = Box::into_raw(trap);

            return None;
        }
    };

    Some(Box::new(wasm_instance_t { inner: instance }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_delete(_instance: Option<Box<wasm_instance_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_exports(
    instance: &wasm_instance_t,
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

    mem::forget(extern_vec);
}
