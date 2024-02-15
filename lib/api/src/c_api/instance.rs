use std::borrow::BorrowMut;
use std::ops::DerefMut;
use std::ptr;
use std::sync::Arc;

use wasmer_types::CompileError;

use crate::bindings::{
    wasm_extern_t, wasm_extern_vec_new, wasm_extern_vec_new_empty,
    wasm_extern_vec_new_uninitialized, wasm_extern_vec_t, wasm_instance_delete,
    wasm_instance_exports, wasm_instance_new, wasm_instance_t, wasm_module_imports, wasm_module_t,
    wasm_store_t, wasm_trap_t,
};
use crate::c_api::vm::VMInstance;
use crate::errors::InstantiationError;
use crate::exports::Exports;
use crate::imports::Imports;
use crate::module::Module;
use crate::store::AsStoreMut;
use crate::Extern;

use super::vm::VMExtern;

#[derive(PartialEq, Eq)]
pub(crate) struct InstanceHandle(pub(crate) *mut wasm_instance_t);

impl InstanceHandle {
    fn new(
        store: *mut wasm_store_t,
        module: *mut wasm_module_t,
        mut externs: Box<[VMExtern]>,
    ) -> Result<Self, InstantiationError> {
        let imports = wasm_extern_vec_t {
            size: externs.len(),
            data: externs.as_mut_ptr(),
        };
        // unsafe { wasm_extern_vec_new(&mut imports, , externs:) };
        let mut trap: *mut wasm_trap_t = std::ptr::null_mut() as _;
        // let mut trap_ptr: *mut wasm_trap_t = &mut trap as *mut _;
        let instance =
            unsafe { wasm_instance_new(store, module, Box::leak(Box::new(imports)), &mut trap) };
        if instance.is_null() {
            return Err(InstantiationError::Start(crate::RuntimeError::new(
                format!("Failed to instantiate"),
            )));
        }
        Ok(InstanceHandle(instance))
    }
    fn get_exports(&self, mut store: &mut impl AsStoreMut, module: &Module) -> Exports {
        let mut exports = wasm_extern_vec_t {
            size: 0,
            data: std::ptr::null_mut() as _,
        };
        // wasm_extern_vec_new_uninitialized(&mut exports, arg1)
        unsafe {
            wasm_instance_exports(self.0, &mut exports);
        }
        // println!("SIZE {}", unsafe { exports.size });
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
        // Exports::default()
        // Exports::from_iter(iter)
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

// Instance can't be Send in js because it dosen't support `structuredClone`
// https://developer.mozilla.org/en-US/docs/Web/API/structuredClone
// unsafe impl Send for Instance {}

impl Instance {
    pub(crate) fn new(
        mut store: &mut impl AsStoreMut,
        module: &Module,
        imports: &Imports,
    ) -> Result<(Self, Exports), InstantiationError> {
        let externs = module
            .imports()
            .map(|import_ty| {
                println!("RETURNING IMPORTS, {:?}", import_ty);
                imports
                    .get_export(import_ty.module(), import_ty.name())
                    .expect("Extern not found")
            })
            .collect::<Vec<_>>();
        Self::new_by_index(store, module, &externs)
    }

    pub(crate) fn new_by_index(
        store: &mut impl AsStoreMut,
        module: &Module,
        externs: &[Extern],
    ) -> Result<(Self, Exports), InstantiationError> {
        let store_ref = store.as_store_ref();
        let externs = externs
            .iter()
            .map(|extern_| extern_.to_vm_extern())
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let instance =
            InstanceHandle::new(store_ref.inner.store.inner, module.0.handle.0, externs)?;
        let exports = instance.get_exports(store, module);
        Ok((
            Self {
                handle: Arc::new(instance),
            },
            exports,
        ))
    }
}
