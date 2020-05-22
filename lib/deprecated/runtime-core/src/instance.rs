use crate::{new, types::Value};
use std::{convert::Infallible, error::Error};

pub struct Instance {
    #[deprecated(
        since = "__NEXT_VERSION__",
        note = "This field is no longer available. Take a look at `wasmer_runtime::InstanceHandle` instead."
    )]
    pub module: (),
    // TODO
    //pub exports: Exports,
    pub(crate) new_instance: new::wasmer::Instance,
}

impl Instance {
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
        use new::wasm_common::entity::EntityRef;

        self.new_instance
            .module()
            .info()
            .resolve_func(name)
            .map(|function_index| function_index.index())
            .ok_or(())
    }

    #[deprecated(since = "__NEXT__VERSION__", note = "Please use `…` instead.")]
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
    pub fn exports(&self) -> &new::wasmer::Exports {
        &self.new_instance.exports
    }
}
