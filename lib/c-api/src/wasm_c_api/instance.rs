use super::externals::{wasm_extern_t, wasm_extern_vec_t};
use super::module::wasm_module_t;
use super::store::wasm_store_t;
use super::trap::wasm_trap_t;
use crate::ordered_resolver::OrderedResolver;
use std::mem;
use std::ptr::NonNull;
use std::sync::Arc;
use wasmer::{Extern, Instance};

#[allow(non_camel_case_types)]
pub struct wasm_instance_t {
    pub(crate) inner: Arc<Instance>,
}

struct CArrayIter<T: Sized + 'static> {
    cur_entry: *const *const T,
}

impl<T: Sized + 'static> CArrayIter<T> {
    fn new(array: *const *const T) -> Option<Self> {
        if array.is_null() {
            None
        } else {
            Some(CArrayIter { cur_entry: array })
        }
    }
}

impl<T: Sized + 'static> Iterator for CArrayIter<T> {
    type Item = &'static T;

    fn next(&mut self) -> Option<Self::Item> {
        let next_entry_candidate = unsafe { *self.cur_entry };
        if next_entry_candidate.is_null() {
            None
        } else {
            self.cur_entry = unsafe { self.cur_entry.add(1) };
            Some(unsafe { &*next_entry_candidate })
        }
    }
}

// reads from null-terminated array of `wasm_extern_t`s
unsafe fn argument_import_iter(
    imports: *const *const wasm_extern_t,
) -> Box<dyn Iterator<Item = &'static wasm_extern_t>> {
    CArrayIter::new(imports)
        .map(|it| Box::new(it) as _)
        .unwrap_or_else(|| Box::new(std::iter::empty()) as _)
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_new(
    store: Option<NonNull<wasm_store_t>>,
    module: &wasm_module_t,
    imports: *const *const wasm_extern_t,
    // own
    _traps: *mut *mut wasm_trap_t,
) -> Option<Box<wasm_instance_t>> {
    let wasm_module = &module.inner;
    let module_imports = wasm_module.imports();
    let module_import_count = module_imports.len();
    let imports = argument_import_iter(imports);
    let resolver: OrderedResolver = imports
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
