use crate::{instance::Instance, new};
use std::error::Error;

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

    pub fn info(&self) -> &new::wasmer_runtime::ModuleInfo {
        &self.new_module.info()
    }
}
