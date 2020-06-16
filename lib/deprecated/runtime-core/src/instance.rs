use crate::{import::LikeNamespace, module::Module, new, structures::TypedIndex, types::Value, vm};
use std::error::Error;

pub use new::wasmer::Exports;

pub(crate) struct PreInstance {
    pub(crate) vmctx: vm::Ctx,
}

impl PreInstance {
    pub(crate) fn new() -> Self {
        Self {
            vmctx: vm::Ctx::new(),
        }
    }

    pub(crate) fn vmctx_ptr(&self) -> *mut vm::Ctx {
        &self.vmctx as *const _ as *mut _
    }
}

// #[derive(Clone)]
pub struct Instance {
    pub exports: Exports,
    pre_instance: Box<PreInstance>,
    pub(crate) new_instance: new::wasmer::Instance,
    // vmctx: vm::Ctx,
}

impl Instance {
    pub(crate) fn new(mut pre_instance: Box<PreInstance>, new_instance: new::wasmer::Instance) -> Self {
        pre_instance.vmctx.x = 42;
        Self {
            exports: new_instance.exports.clone(),
            new_instance,
            pre_instance,
        }
    }

    pub fn load<T>(&self, _loader: T) -> Result<Self, ()> {
        Err(())
    }

    pub fn resolve_func(&self, name: &str) -> Result<usize, ()> {
        self.new_instance
            .module()
            .info()
            .func_names
            .iter()
            .find_map(|(function_index, function_name)| {
                if function_name.as_str() == name {
                    Some(function_index)
                } else {
                    None
                }
            })
            .map(|function_index| function_index.index())
            .ok_or(())
    }

    pub fn call(&self, name: &str, params: &[Value]) -> Result<Vec<Value>, Box<dyn Error>> {
        Ok(self
            .new_instance
            .exports
            .get_function(name)?
            .call(params)?
            .into_vec())
    }

    pub fn module(&self) -> Module {
        Module::new(self.new_instance.module().clone())
    }

    pub fn context(&self) -> &vm::Ctx {
        &self.pre_instance.vmctx
    }

    pub fn context_mut(&mut self) -> &mut vm::Ctx {
        &mut self.pre_instance.vmctx
    }
}

impl LikeNamespace for Instance {
    fn get_namespace_export(&self, name: &str) -> Option<new::wasmer_runtime::Export> {
        self.exports.get_namespace_export(name)
    }

    fn get_namespace_exports(&self) -> Vec<(String, new::wasmer_runtime::Export)> {
        self.exports.get_namespace_exports()
    }
}
