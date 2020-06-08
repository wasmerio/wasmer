use crate::{cache::Artifact, module::Module, new};
use std::{convert::Infallible, error::Error};

pub fn compile(bytes: &[u8]) -> Result<Module, Box<dyn Error>> {
    compile_with(bytes, ())
}

pub fn load_cache_with(cache: Artifact) -> Result<Module, Infallible> {
    Ok(cache.module())
}

#[deprecated(
    since = "__NEXT_VERSION__",
    note = "Please use `Module::new(&store, bytes)` instead."
)]
pub fn compile_with(bytes: &[u8], _compiler: ()) -> Result<Module, Box<dyn Error>> {
    let store = Default::default();

    Ok(Module::new(new::wasmer::Module::new(&store, bytes)?))
}

#[deprecated(
    since = "__NEXT_VERSION__",
    note = "Please use `Module::new(&store, bytes)` instead."
)]
pub fn compile_with_config(
    bytes: &[u8],
    _compiler: (),
    _compiler_config: (),
) -> Result<Module, Box<dyn Error>> {
    let store = Default::default();

    Ok(Module::new(new::wasmer::Module::new(&store, bytes)?))
}

#[deprecated(
    since = "__NEXT_VERSION__",
    note = "Please use `Module::validate(&store, bytes)` instead."
)]
pub fn validate(bytes: &[u8]) -> bool {
    let store = Default::default();

    new::wasmer::Module::validate(&store, bytes).is_ok()
}

#[macro_export]
macro_rules! func {
    ($function:expr) => {
        $crate::typed_func::Func::new($function)
    };
}
