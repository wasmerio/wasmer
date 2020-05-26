use crate::{module::Module, new, structures::TypedIndex, types::Value};
use std::{convert::Infallible, error::Error};

pub use new::wasmer::Exports;

pub struct Instance {
    #[deprecated(
        since = "__NEXT_VERSION__",
        note = "This field is no longer available. Take a look at `wasmer_runtime::InstanceHandle` instead."
    )]
    pub module: (),
    pub(crate) new_instance: new::wasmer::Instance,
}

impl Instance {
    pub(crate) fn new(new_instance: new::wasmer::Instance) -> Self {
        Self {
            module: (),
            new_instance,
        }
    }

    #[deprecated(
        since = "__NEXT__VERSION__",
        note = "This method is no longer available."
    )]
    pub fn load<T>(&self, _loader: T) -> Result<Self, ()> {
        Err(())
    }

    #[deprecated(
        since = "__NEXT__VERSION__",
        note = "This method is no longer available."
    )]
    pub fn func<Args, Rets>(&self, _name: &str) -> Result<Infallible, ()> {
        Err(())
    }

    #[deprecated(
        since = "__NEXT__VERSION__",
        note = "Please use `instance.module().info().resolve_func(name)` instead."
    )]
    pub fn resolve_func(&self, name: &str) -> Result<usize, ()> {
        self.new_instance
            .module()
            .info()
            .resolve_func(name)
            .map(|function_index| function_index.index())
            .ok_or(())
    }

    #[deprecated(
        since = "__NEXT__VERSION__",
        note = "Please use `instance.exports.get_function(name).call(params)` instead."
    )]
    pub fn call(&self, name: &str, params: &[Value]) -> Result<Vec<Value>, Box<dyn Error>> {
        Ok(self
            .new_instance
            .exports
            .get_function(name)?
            .call(params)?
            .into_vec())
    }

    #[deprecated(
        since = "__NEXT_VERSION__",
        note = "Please use `instance.exports` instead."
    )]
    pub fn exports(&self) -> &Exports {
        &self.new_instance.exports
    }

    pub fn module(&self) -> Module {
        Module::new(self.new_instance.module().clone())
    }
}
