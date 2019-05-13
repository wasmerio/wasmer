use crate::signal::Caller;
use std::sync::Arc;
use wasmer_runtime_core::{
    backend::{Backend, CacheGen, Token},
    cache::{Artifact, Error as CacheError},
    codegen::*,
    module::{ModuleInfo, ModuleInner},
    structures::Map,
    types::{FuncIndex, FuncSig, SigIndex},
};
use wasmparser::Type as WpType;

pub struct CraneliftModuleCodeGenerator {
    signatures: Option<Arc<Map<SigIndex, FuncSig>>>,
    function_signatures: Option<Arc<Map<FuncIndex, SigIndex>>>,
    functions: Vec<CraneliftFunctionCodeGenerator>,
}

impl ModuleCodeGenerator<CraneliftFunctionCodeGenerator, Caller, CodegenError>
    for CraneliftModuleCodeGenerator
{
    fn new() -> Self {
        CraneliftModuleCodeGenerator {
            functions: vec![],
            function_signatures: None,
            signatures: None,
        }
    }

    fn backend_id() -> Backend {
        Backend::Cranelift
    }

    fn check_precondition(&mut self, _module_info: &ModuleInfo) -> Result<(), CodegenError> {
        Ok(())
    }

    fn next_function(&mut self) -> Result<&mut CraneliftFunctionCodeGenerator, CodegenError> {
        let code = CraneliftFunctionCodeGenerator {};
        self.functions.push(code);
        Ok(self.functions.last_mut().unwrap())
    }

    fn finalize(
        self,
        _module_info: &ModuleInfo,
    ) -> Result<(Caller, Box<dyn CacheGen>), CodegenError> {
        unimplemented!()
    }

    fn feed_signatures(&mut self, signatures: Map<SigIndex, FuncSig>) -> Result<(), CodegenError> {
        self.signatures = Some(Arc::new(signatures));
        Ok(())
    }

    fn feed_function_signatures(
        &mut self,
        assoc: Map<FuncIndex, SigIndex>,
    ) -> Result<(), CodegenError> {
        self.function_signatures = Some(Arc::new(assoc));
        Ok(())
    }

    fn feed_import_function(&mut self) -> Result<(), CodegenError> {
        Ok(())
    }

    unsafe fn from_cache(_cache: Artifact, _: Token) -> Result<ModuleInner, CacheError> {
        unimplemented!()
    }
}

pub struct CraneliftFunctionCodeGenerator {}

impl FunctionCodeGenerator<CodegenError> for CraneliftFunctionCodeGenerator {
    fn feed_return(&mut self, _ty: WpType) -> Result<(), CodegenError> {
        Ok(())
    }

    fn feed_param(&mut self, _ty: WpType) -> Result<(), CodegenError> {
        Ok(())
    }

    fn feed_local(&mut self, _ty: WpType, _n: usize) -> Result<(), CodegenError> {
        Ok(())
    }

    fn begin_body(&mut self, _module_info: &ModuleInfo) -> Result<(), CodegenError> {
        Ok(())
    }

    fn feed_event(&mut self, _op: Event, _module_info: &ModuleInfo) -> Result<(), CodegenError> {
        Ok(())
    }

    fn finalize(&mut self) -> Result<(), CodegenError> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct CodegenError {
    pub message: String,
}
