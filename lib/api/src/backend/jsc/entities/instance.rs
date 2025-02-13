use crate::{
    jsc::{utils::convert::AsJsc, vm::VMInstance},
    AsStoreMut, BackendInstance, Exports, Extern, Imports, InstantiationError, Module,
};

use super::engine::IntoJSC;

#[derive(Clone, PartialEq, Eq)]
pub struct Instance {
    pub(crate) _handle: VMInstance,
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
        let instance = module
            .as_jsc()
            .instantiate(&mut store, imports)
            .map_err(|e| InstantiationError::Start(e))?;

        Self::from_module_and_instance(store, &module, instance)
    }

    pub(crate) fn new_by_index(
        store: &mut impl AsStoreMut,
        module: &Module,
        externs: &[Extern],
    ) -> Result<(Self, Exports), InstantiationError> {
        let mut imports = Imports::new();
        for (import_ty, extern_ty) in module.imports().zip(externs.iter()) {
            imports.define(import_ty.module(), import_ty.name(), extern_ty.clone());
        }
        Self::new(store, module, &imports)
    }

    pub(crate) fn from_module_and_instance(
        mut store: &mut impl AsStoreMut,
        module: &Module,
        instance: VMInstance,
    ) -> Result<(Self, Exports), InstantiationError> {
        let engine = store.as_store_mut();
        let context = engine.jsc().context();

        let mut instance_exports = instance
            .get_property(&context, "exports".to_string())
            .to_object(&context)
            .unwrap();

        let exports_ty = module.exports().collect::<Vec<_>>();

        let exports = exports_ty
            .iter()
            .map(|export_type| {
                let name = export_type.name();
                let mut store = store.as_store_mut();
                let context = store.jsc().context();
                let extern_type = export_type.ty();
                // Annotation is here to prevent spurious IDE warnings.
                let js_export = instance_exports.get_property(&context, name.to_string());
                let extern_ = Extern::from_jsc_value(&mut store, extern_type, &js_export).unwrap();
                Ok((name.to_string(), extern_))
            })
            .collect::<Result<Exports, InstantiationError>>()?;

        Ok((Self { _handle: instance }, exports))
    }
}

impl crate::Instance {
    /// Consume [`self`] into [`crate::backend::jsc::instance::Instance`].
    pub fn into_jsc(self) -> crate::backend::jsc::instance::Instance {
        match self._inner {
            BackendInstance::Jsc(s) => s,
            _ => panic!("Not a `jsc` global!"),
        }
    }

    /// Convert a reference to [`self`] into a reference to [`crate::backend::jsc::instance::Instance`].
    pub fn as_jsc(&self) -> &crate::backend::jsc::instance::Instance {
        match self._inner {
            BackendInstance::Jsc(ref s) => s,
            _ => panic!("Not a `jsc` global!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference to [`crate::backend::jsc::instance::Instance`].
    pub fn as_jsc_mut(&mut self) -> &mut crate::backend::jsc::instance::Instance {
        match self._inner {
            BackendInstance::Jsc(ref mut s) => s,
            _ => panic!("Not a `jsc` global!"),
        }
    }
}
