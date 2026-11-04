//! Source preparation at initial/exec compilation; custom embedders can call this too.
use wasixv2_exec::{Image, LoadError, Loader};
use wasixv2_syscalls::LocalFuture;
use wasmer::{CompileError, Engine, Module};
pub fn load(engine: &Engine, source: &[u8]) -> Result<Module, CompileError> {
    let prepared =
        crate::prepare_image(source).map_err(|error| CompileError::Validate(error.to_string()))?;
    if prepared == source {
        return wasixv2_exec::load(engine, source);
    }
    let prepared = wasixv2_exec::instrument(&prepared)
        .map_err(|error| CompileError::Validate(error.to_string()))?;
    Module::new(engine, prepared)
}
pub struct Compile;
impl Loader for Compile {
    fn load<'a>(
        &'a self,
        engine: &'a Engine,
        image: &'a Image,
    ) -> LocalFuture<'a, Result<(Engine, Module), LoadError>> {
        Box::pin(async move { Ok((engine.clone(), load(engine, image.bytes())?)) })
    }
}
