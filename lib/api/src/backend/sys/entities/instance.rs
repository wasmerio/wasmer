//! Data types, functions and traits for `sys` runtime's `Instance` implementation.

use crate::{
    error::InstantiationError, exports::Exports, imports::Imports, module::Module,
    store::AsStoreMut, Extern,
};
use wasmer_vm::{StoreHandle, VMInstance};

use super::store::Store;

#[derive(Clone, PartialEq, Eq)]
/// A WebAssembly `instance` in the `sys` runtime.
pub struct Instance {
    _handle: StoreHandle<VMInstance>,
}

#[cfg(test)]
mod send_test {
    use super::*;

    // Only here to statically ensure that `Instance` is `Send`.
    // Will fail to compile otherwise.
    #[allow(dead_code)]
    fn instance_is_send(inst: Instance) {
        fn is_send(t: impl Send) {
            let _ = t;
        }

        is_send(inst);
    }
}

impl From<wasmer_compiler::InstantiationError> for InstantiationError {
    fn from(other: wasmer_compiler::InstantiationError) -> Self {
        match other {
            wasmer_compiler::InstantiationError::Link(e) => Self::Link(e.into()),
            wasmer_compiler::InstantiationError::Start(e) => Self::Start(e.into()),
            wasmer_compiler::InstantiationError::CpuFeature(e) => Self::CpuFeature(e),
        }
    }
}

impl Instance {
    #[allow(clippy::result_large_err)]
    pub(crate) fn new(
        store: &mut impl AsStoreMut,
        module: &Module,
        imports: &Imports,
    ) -> Result<(Self, Exports), InstantiationError> {
        let externs = imports
            .imports_for_module(module)
            .map_err(InstantiationError::Link)?;
        let mut handle = module.as_sys().instantiate(store, &externs)?;
        let exports = Self::get_exports(store, module, handle.as_sys_mut());

        let instance = Self {
            _handle: StoreHandle::new(store.objects_mut().as_sys_mut(), handle.into_sys()),
        };

        Ok((instance, exports))
    }

    #[allow(clippy::result_large_err)]
    pub(crate) fn new_by_index(
        store: &mut impl AsStoreMut,
        module: &Module,
        externs: &[Extern],
    ) -> Result<(Self, Exports), InstantiationError> {
        let externs = externs.to_vec();
        let mut handle = module.as_sys().instantiate(store, &externs)?;
        let exports = Self::get_exports(store, module, handle.as_sys_mut());
        let instance = Self {
            _handle: StoreHandle::new(
                store.as_store_mut().objects_mut().as_sys_mut(),
                handle.into_sys(),
            ),
        };

        Ok((instance, exports))
    }

    fn get_exports(
        store: &mut impl AsStoreMut,
        module: &Module,
        handle: &mut VMInstance,
    ) -> Exports {
        module
            .exports()
            .map(|export| {
                let name = export.name().to_string();
                let export = handle.lookup(&name).expect("export");
                let extern_ = Extern::from_vm_extern(store, crate::vm::VMExtern::Sys(export));
                (name, extern_)
            })
            .collect::<Exports>()
    }
}

impl crate::BackendInstance {
    /// Consume [`self`] into a [`crate::backend::sys::instance::Instance`].
    pub(crate) fn into_sys(self) -> crate::backend::sys::instance::Instance {
        match self {
            Self::Sys(s) => s,
            _ => panic!("Not a `sys` instance"),
        }
    }
}
