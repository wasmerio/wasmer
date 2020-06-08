use crate::{module::Module, new, structures::TypedIndex, types::Value};
use std::{convert::Infallible, error::Error};

pub use new::wasmer::Exports;

#[derive(Clone)]
pub struct Instance {
    pub exports: Exports,
    pub(crate) new_instance: new::wasmer::Instance,
}

impl Instance {
    pub(crate) fn new(new_instance: new::wasmer::Instance) -> Self {
        Self {
            exports: new_instance.exports.clone(),
            new_instance,
        }
    }

    pub fn load<T>(&self, _loader: T) -> Result<Self, ()> {
        Err(())
    }

    pub fn func<Args, Rets>(&self, _name: &str) -> Result<Infallible, ()> {
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

    pub fn exports(&self) -> Exports {
        self.exports.clone()
    }

    pub fn module(&self) -> Module {
        Module::new(self.new_instance.module().clone())
    }
}
