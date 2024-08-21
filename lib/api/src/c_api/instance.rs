use std::alloc::Layout;
use std::borrow::BorrowMut;
use std::mem::{size_of, size_of_val};
use std::ops::DerefMut;
use std::sync::Arc;
use std::{mem, ptr};

use wasmer_types::CompileError;

use crate::bindings::{
    wasm_extern_t, wasm_extern_vec_new, wasm_extern_vec_new_empty,
    wasm_extern_vec_new_uninitialized, wasm_extern_vec_t, wasm_instance_delete,
    wasm_instance_exports, wasm_instance_new, wasm_instance_new_with_args, wasm_instance_t,
    wasm_module_imports, wasm_module_t, wasm_runtime_init_thread_env, wasm_runtime_instantiate,
    wasm_runtime_thread_env_inited, wasm_store_t, wasm_trap_t,
};
use crate::c_api::vm::VMInstance;
use crate::errors::InstantiationError;
use crate::exports::Exports;
use crate::imports::Imports;
use crate::module::Module;
use crate::store::AsStoreMut;
use crate::trap::Trap;
use crate::Extern;

use super::vm::VMExtern;

#[derive(PartialEq, Eq)]
pub(crate) struct InstanceHandle(pub(crate) *mut wasm_instance_t);

unsafe impl Send for InstanceHandle {}
unsafe impl Sync for InstanceHandle {}

impl InstanceHandle {
    fn new(
        store: *mut wasm_store_t,
        module: *mut wasm_module_t,
        mut externs: Vec<VMExtern>,
    ) -> Result<Self, InstantiationError> {
        // Check if the thread env was already initialised.
        unsafe {
            if !wasm_runtime_thread_env_inited() {
                if !wasm_runtime_init_thread_env() {
                    panic!("Failed to initialize the thread environment!");
                }
            }
        }

        let mut imports = unsafe {
            let mut vec = Default::default();
            wasm_extern_vec_new(&mut vec, externs.len(), externs.as_ptr());
            vec
        };

        std::mem::forget(externs);

        let mut trap: *mut wasm_trap_t = std::ptr::null_mut() as _;

        let instance = unsafe {
            let stack_size = 2 * 1024 * 1024;
            let heap_size = 2 * 1024 * 1024;

            wasm_instance_new_with_args(
                store,
                module,
                &mut imports,
                &mut trap,
                stack_size,
                heap_size,
            )
        };

        if instance.is_null() {
            let trap = Trap::from(trap);
        }

        Ok(InstanceHandle(instance))
    }

    fn get_exports(&self, mut store: &mut impl AsStoreMut, module: &Module) -> Exports {
        let mut exports = unsafe {
            let mut vec = Default::default();
            wasm_instance_exports(self.0, &mut vec);
            vec
        };

        let wasm_exports: &[*mut wasm_extern_t] =
            unsafe { std::slice::from_raw_parts(exports.data, exports.size) };

        let exports_ty = module.exports().collect::<Vec<_>>();
        let exports = exports_ty
            .iter()
            .zip(wasm_exports.into_iter())
            .map(|(export_type, wasm_export)| {
                let name = export_type.name();
                let mut store = store.as_store_mut();
                let extern_type = export_type.ty();
                // Annotation is here to prevent spurious IDE warnings.

                let extern_ = Extern::from_vm_extern(&mut store, *wasm_export);
                (name.to_string(), extern_)
            })
            .collect::<Exports>();
        exports
    }
}
impl Drop for InstanceHandle {
    fn drop(&mut self) {
        unsafe { wasm_instance_delete(self.0) }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Instance {
    pub(crate) handle: Arc<InstanceHandle>,
}

impl Instance {
    pub(crate) fn new(
        _store: &mut impl AsStoreMut,
        module: &Module,
        imports: &Imports,
    ) -> Result<(Self, Exports), InstantiationError> {
        let externs = module
            .imports()
            .map(|import_ty| {
                imports
                    .get_export(import_ty.module(), import_ty.name())
                    .expect("Extern not found")
            })
            .collect::<Vec<_>>();

        // Hacky: we need to tie a *module* to a store before instantiating it..
        let mut store_from_module = module.0.handle.store.lock().unwrap();
        let mut store = store_from_module.as_store_mut();

        Self::new_by_index(&mut store, module, &externs)
    }

    pub(crate) fn new_by_index(
        store: &mut impl AsStoreMut,
        module: &Module,
        externs: &[Extern],
    ) -> Result<(Self, Exports), InstantiationError> {
        let store_ref = store.as_store_ref();
        let externs: Vec<*mut wasm_extern_t> = externs
            .iter()
            .map(|extern_| {
                let vm_extern = extern_.to_vm_extern();
                vm_extern
            })
            .collect::<Vec<_>>();
        let instance =
            InstanceHandle::new(store_ref.inner.store.inner, module.0.handle.inner, externs)?;
        let exports = instance.get_exports(store, module);

        Ok((
            Self {
                handle: Arc::new(instance),
            },
            exports,
        ))
    }
}
