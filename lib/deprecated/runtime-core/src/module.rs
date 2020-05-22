use crate::{instance::Instance, new};

pub struct Module {
    pub(crate) new_module: new::wasmer::Module,
}

impl Module {
    pub(crate) fn new(new_module: new::wasmer::Module) -> Self {
        Self { new_module }
    }

    pub fn info(&self) -> &new::wasmer_runtime::ModuleInfo {
        &self.new_module.info()
    }
}
