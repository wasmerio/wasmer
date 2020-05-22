mod new {
    pub(crate) use wasm_common::entity::EntityRef;
    pub(crate) use wasmer::Instance;
}

use std::convert::Infallible;

pub struct Instance {
    #[deprecated(
        since = "__NEXT_VERSION__",
        note = "This field is no longer available. Take a look at `wasmer_runtime::InstanceHandle` instead."
    )]
    pub module: (),
    // TODO
    //pub exports: Exports,
    pub(crate) new_instance: new::Instance,
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
        note = "Please use `instance.exports.get_function(name)."
    )]
    pub fn resolve_func(&self, name: &str) -> Result<usize, ()> {
        use self::new::EntityRef;

        self.new_instance
            .module()
            .info()
            .func_names
            .iter()
            .find_map(|(function_index, function_name)| {
                if function_name.as_str() == name {
                    Some(function_index.index())
                } else {
                    None
                }
            })
            .ok_or(())
    }
}
