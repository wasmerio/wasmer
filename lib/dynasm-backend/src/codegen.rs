use wasmer_runtime_core::{
    backend::{FuncResolver, ProtectedCaller},
    module::ModuleInfo,
    structures::Map,
    types::{FuncIndex, FuncSig, SigIndex},
};
use wasmparser::{Operator, Type as WpType};

pub trait ModuleCodeGenerator<FCG: FunctionCodeGenerator, PC: ProtectedCaller, FR: FuncResolver> {
    fn check_precondition(&mut self, module_info: &ModuleInfo) -> Result<(), CodegenError>;
    fn next_function(&mut self) -> Result<&mut FCG, CodegenError>;
    fn finalize(self, module_info: &ModuleInfo) -> Result<(PC, FR), CodegenError>;
    fn feed_signatures(&mut self, signatures: Map<SigIndex, FuncSig>) -> Result<(), CodegenError>;
    fn feed_function_signatures(
        &mut self,
        assoc: Map<FuncIndex, SigIndex>,
    ) -> Result<(), CodegenError>;
    fn feed_import_function(&mut self) -> Result<(), CodegenError>;
}

pub trait FunctionCodeGenerator {
    fn feed_return(&mut self, ty: WpType) -> Result<(), CodegenError>;
    fn feed_param(&mut self, ty: WpType) -> Result<(), CodegenError>;
    fn feed_local(&mut self, ty: WpType, n: usize) -> Result<(), CodegenError>;
    fn begin_body(&mut self) -> Result<(), CodegenError>;
    fn feed_opcode(&mut self, op: Operator, module_info: &ModuleInfo) -> Result<(), CodegenError>;
    fn finalize(&mut self) -> Result<(), CodegenError>;
}

#[derive(Debug)]
pub struct CodegenError {
    pub message: &'static str,
}
