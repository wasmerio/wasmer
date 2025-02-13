//! Data types, functions and traits for `wamr`'s `Instance` implementation.
use std::sync::Arc;

use crate::{
    backend::wamr::bindings::*, vm::VMExtern, wamr::error::Trap, AsStoreMut, AsStoreRef, Exports,
    Extern, Imports, InstantiationError, Module,
};

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
        //unsafe {
        //    if !wasm_runtime_thread_env_inited() {
        //        if !wasm_runtime_init_thread_env() {
        //            panic!("Failed to initialize the thread environment!");
        //        }
        //    }
        //}

        let mut trap: *mut wasm_trap_t = std::ptr::null_mut() as _;
        let externs: Vec<_> = externs.into_iter().map(|v| v.into_wamr()).collect();

        let instance = unsafe {
            let mut imports = unsafe {
                let mut vec = Default::default();
                wasm_extern_vec_new(&mut vec, externs.len(), externs.as_ptr());
                vec
            };

            std::mem::forget(externs);

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
            return Err(InstantiationError::Start(trap.into()));
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

                let extern_ = Extern::from_vm_extern(&mut store, VMExtern::Wamr(*wasm_export));
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
/// A WebAssembly `instance` in `wamr`.
pub struct Instance {
    pub(crate) handle: Arc<InstanceHandle>,
}

impl Instance {
    pub(crate) fn new(
        store: &mut impl AsStoreMut,
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

        _ = store;
        // Hacky: we need to tie a *module* to a store before instantiating it..
        let wamr_module = module.as_wamr();
        let mut store_from_module = wamr_module.handle.store.lock().unwrap();
        let mut store = store_from_module.as_store_mut();

        return Self::new_by_index(&mut store, module, &externs);
    }

    pub(crate) fn new_by_index(
        store: &mut impl AsStoreMut,
        module: &Module,
        externs: &[Extern],
    ) -> Result<(Self, Exports), InstantiationError> {
        let store_ref = store.as_store_ref();
        let externs: Vec<VMExtern> = externs
            .iter()
            .map(|extern_| extern_.to_vm_extern())
            .collect::<Vec<_>>();

        let instance = InstanceHandle::new(
            store_ref.inner.store.as_wamr().inner,
            module.as_wamr().handle.inner,
            externs,
        )?;
        let exports = instance.get_exports(store, module);

        Ok((
            Self {
                handle: Arc::new(instance),
            },
            exports,
        ))
    }
}

impl crate::BackendInstance {
    /// Consume [`self`] into a [`crate::backend::wamr::instance::Instance`].
    pub(crate) fn into_wamr(self) -> crate::backend::wamr::instance::Instance {
        match self {
            Self::Wamr(s) => s,
            _ => panic!("Not a `wamr` instance"),
        }
    }
}
