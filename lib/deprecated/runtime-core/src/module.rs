use crate::{
    cache::Artifact,
    error::InstantiationError,
    instance::{Instance, PreInstance},
    new, vm,
};
use new::wasmer_runtime::Export;
use std::convert::{AsRef, Infallible};

pub use new::wasm_common::{DataInitializer, ExportIndex};
pub use new::wasmer_runtime::{
    //
    MemoryStyle as MemoryType,
    ModuleInfo,
    TableElements as TableInitializer,
};

#[derive(Clone)]
pub struct Module {
    pub(crate) new_module: new::wasmer::Module,
}

impl Module {
    pub(crate) fn new(new_module: new::wasmer::Module) -> Self {
        Self { new_module }
    }

    pub fn instantiate(
        &self,
        import_object: &crate::import::ImportObject,
    ) -> Result<Instance, InstantiationError> {
        let pre_instance = PreInstance::new();
        dbg!(pre_instance.vmctx_ptr());
        dbg!({
            let vmctx: &vm::Ctx = unsafe { &*pre_instance.vmctx_ptr() };
            vmctx
        });

        // Replace the fake `vm::Ctx` of host functions.
        {
            let import_object = import_object.clone_ref();

            import_object
                .into_iter()
                .filter_map(|(_, export)| match export {
                    Export::Function(function) => Some(function),
                    _ => None,
                })
                .for_each(|mut exported_function| {
                    // Cast `vm::Ctx` (from this crate) to `VMContext`
                    // (from `wasmer_runtime`).
                    dbg!(exported_function.vmctx);

                    if exported_function.vmctx.is_null() {
                        exported_function.vmctx = pre_instance.vmctx_ptr() as _;
                    }

                    dbg!(exported_function.vmctx);
                });
        }

        Ok(Instance::new(
            pre_instance,
            new::wasmer::Instance::new(&self.new_module, import_object)?,
        ))
    }

    pub fn cache(&self) -> Result<Artifact, Infallible> {
        Ok(Artifact::new(self.new_module.clone()))
    }

    pub fn info(&self) -> &ModuleInfo {
        &self.new_module.info()
    }

    pub fn imports(&self) -> Vec<crate::types::ImportDescriptor> {
        self.new_module.imports().collect()
    }

    pub fn exports(&self) -> Vec<crate::types::ExportDescriptor> {
        self.new_module.exports().collect()
    }

    pub fn custom_sections<'a>(&self, name: impl AsRef<str>) -> Option<Vec<Vec<u8>>> {
        let custom_sections: Vec<Vec<u8>> = self
            .new_module
            .custom_sections(name.as_ref())
            .map(|custom_section| custom_section.to_vec())
            .collect();

        if custom_sections.is_empty() {
            None
        } else {
            Some(custom_sections)
        }
    }
}
