use crate::{cache::Artifact, instance::Instance, new};
use std::{convert::Infallible, error::Error};

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
}
