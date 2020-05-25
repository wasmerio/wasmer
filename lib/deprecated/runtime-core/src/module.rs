use crate::{cache::Artifact, instance::Instance, new};
use std::{convert::Infallible, error::Error};

pub use new::wasm_common::ExportIndex;

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
    ) -> Result<Instance, Box<dyn Error>> {
        Ok(Instance::new(new::wasmer::Instance::new(
            &self.new_module,
            import_object,
        )?))
    }

    pub fn cache(&self) -> Result<Artifact, Infallible> {
        Ok(Artifact::new(self.new_module.clone()))
    }

    pub fn info(&self) -> &new::wasmer_runtime::ModuleInfo {
        &self.new_module.info()
    }

    pub fn imports(&self) -> Vec<crate::types::ImportDescriptor> {
        self.new_module.imports().collect()
    }

    pub fn exports(&self) -> Vec<crate::types::ExportDescriptor> {
        self.new_module.exports().collect()
    }

    pub fn custom_sections<'a>(&self, name: &'a str) -> Option<Vec<Vec<u8>>> {
        let custom_sections: Vec<Vec<u8>> = self
            .new_module
            .custom_sections(name)
            .map(|custom_section| custom_section.to_vec())
            .collect();

        if custom_sections.is_empty() {
            None
        } else {
            Some(custom_sections)
        }
    }
}
