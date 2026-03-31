//! Data types, functions and traits for `wasmi`'s `Instance` implementation.
use std::sync::Arc;

use ::wasmi as wasmi_native;

use crate::{
    AsStoreMut, Exports, Extern, Imports, InstantiationError, Module,
    VMExternToExtern,
    vm::VMExtern,
};

#[derive(Clone, Copy)]
pub(crate) struct InstanceHandle(pub(crate) wasmi_native::Instance);

unsafe impl Send for InstanceHandle {}
unsafe impl Sync for InstanceHandle {}

impl PartialEq for InstanceHandle {
    fn eq(&self, other: &Self) -> bool {
        crate::backend::wasmi::vm::handle_bits(self.0)
            == crate::backend::wasmi::vm::handle_bits(other.0)
    }
}

impl Eq for InstanceHandle {}

fn to_native_extern(extern_: VMExtern) -> wasmi_native::Extern {
    match extern_ {
        VMExtern::Wasmi(extern_) => match extern_ {
            crate::backend::wasmi::vm::VMExtern::Function(f) => wasmi_native::Extern::Func(f),
            crate::backend::wasmi::vm::VMExtern::Global(g) => wasmi_native::Extern::Global(g),
            crate::backend::wasmi::vm::VMExtern::Memory(m) => wasmi_native::Extern::Memory(m),
            crate::backend::wasmi::vm::VMExtern::Table(t) => wasmi_native::Extern::Table(t),
        },
        _ => panic!("cross-backend imports are not supported for wasmi"),
    }
}

impl InstanceHandle {
    fn new(
        store: &mut crate::backend::wasmi::store::Store,
        module: &crate::backend::wasmi::module::Module,
        externs: Vec<VMExtern>,
    ) -> Result<Self, InstantiationError> {
        let imports = externs.into_iter().map(to_native_extern).collect::<Vec<_>>();
        let instance = wasmi_native::Instance::new(&mut store.inner, &module.handle.inner, &imports)
            .map_err(|err| InstantiationError::Start(crate::backend::wasmi::error::Trap::from_wasmi_error(err)))?;
        Ok(Self(instance))
    }

    fn get_exports(
        &self,
        store: &mut impl AsStoreMut,
        module: &Module,
    ) -> Exports {
        let mut exports = Exports::new();
        for export in module.exports() {
            if let Some(ext) = self
                .0
                .get_export(&store.as_store_ref().inner.store.as_wasmi().inner, export.name())
            {
                let vm_extern = match ext {
                    wasmi_native::Extern::Func(f) => {
                        VMExtern::Wasmi(crate::backend::wasmi::vm::VMExtern::Function(f))
                    }
                    wasmi_native::Extern::Global(g) => {
                        VMExtern::Wasmi(crate::backend::wasmi::vm::VMExtern::Global(g))
                    }
                    wasmi_native::Extern::Memory(m) => {
                        VMExtern::Wasmi(crate::backend::wasmi::vm::VMExtern::Memory(m))
                    }
                    wasmi_native::Extern::Table(t) => {
                        VMExtern::Wasmi(crate::backend::wasmi::vm::VMExtern::Table(t))
                    }
                };
                exports.insert(export.name().to_string(), vm_extern.to_extern(store));
            }
        }
        exports
    }
}

#[derive(Clone, PartialEq, Eq)]
/// A WebAssembly `instance` in `wasmi`.
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

        Self::new_by_index(store, module, &externs)
    }

    pub(crate) fn new_by_index(
        store: &mut impl AsStoreMut,
        module: &Module,
        externs: &[Extern],
    ) -> Result<(Self, Exports), InstantiationError> {
        let externs = externs.iter().map(|extern_| extern_.to_vm_extern()).collect::<Vec<_>>();
        let instance = {
            let store_mut = store.as_store_mut();
            InstanceHandle::new(store_mut.inner.store.as_wasmi_mut(), module.as_wasmi(), externs)?
        };
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
    pub(crate) fn into_wasmi(self) -> crate::backend::wasmi::instance::Instance {
        match self {
            Self::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` instance"),
        }
    }
}
