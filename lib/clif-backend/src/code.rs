use crate::signal::Caller;
use wasmer_runtime_core::{
    backend::{Backend, CacheGen, Token},
    cache::{Artifact, Error as CacheError},
    codegen::*,
    module::{ModuleInfo, ModuleInner},
    structures::Map,
    types::{FuncIndex, FuncSig, SigIndex},
};
use wasmparser::Type as WpType;

pub struct CraneliftModuleCodeGenerator {}

impl ModuleCodeGenerator<CraneliftFunctionCodeGenerator, Caller, CodegenError>
    for CraneliftModuleCodeGenerator
{
    fn new() -> Self {
        unimplemented!()
    }

    fn backend_id() -> Backend {
        unimplemented!()
    }

    fn check_precondition(&mut self, _module_info: &ModuleInfo) -> Result<(), CodegenError> {
        unimplemented!()
    }

    fn next_function(&mut self) -> Result<&mut CraneliftFunctionCodeGenerator, CodegenError> {
        unimplemented!()
    }

    fn finalize(
        self,
        _module_info: &ModuleInfo,
    ) -> Result<(Caller, Box<dyn CacheGen>), CodegenError> {
        unimplemented!()
    }

    fn feed_signatures(&mut self, _signatures: Map<SigIndex, FuncSig>) -> Result<(), CodegenError> {
        unimplemented!()
    }

    fn feed_function_signatures(
        &mut self,
        _assoc: Map<FuncIndex, SigIndex>,
    ) -> Result<(), CodegenError> {
        unimplemented!()
    }

    fn feed_import_function(&mut self) -> Result<(), CodegenError> {
        unimplemented!()
    }

    unsafe fn from_cache(_cache: Artifact, _: Token) -> Result<ModuleInner, CacheError> {
        unimplemented!()
    }
}

pub struct CraneliftFunctionCodeGenerator {}

impl FunctionCodeGenerator<CodegenError> for CraneliftFunctionCodeGenerator {
    fn feed_return(&mut self, _ty: WpType) -> Result<(), CodegenError> {
        unimplemented!()
    }

    fn feed_param(&mut self, _ty: WpType) -> Result<(), CodegenError> {
        unimplemented!()
    }

    fn feed_local(&mut self, _ty: WpType, _n: usize) -> Result<(), CodegenError> {
        unimplemented!()
    }

    fn begin_body(&mut self, _module_info: &ModuleInfo) -> Result<(), CodegenError> {
        unimplemented!()
    }

    fn feed_event(&mut self, _op: Event, _module_info: &ModuleInfo) -> Result<(), CodegenError> {
        unimplemented!()
    }

    fn finalize(&mut self) -> Result<(), CodegenError> {
        unimplemented!()
    }
}

#[derive(Debug)]
pub struct CodegenError {
    pub message: String,
}
