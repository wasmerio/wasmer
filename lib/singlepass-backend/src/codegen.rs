use wasmer_runtime_core::{
    backend::{FuncResolver, ProtectedCaller},
    module::ModuleInfo,
    structures::Map,
    types::{FuncIndex, FuncSig, SigIndex},
};
use wasmparser::{Operator, Type as WpType};

/// The module-scope code generator trait.
pub trait ModuleCodeGenerator<FCG: FunctionCodeGenerator, PC: ProtectedCaller, FR: FuncResolver> {
    /// Verifies that the module satisfies a precondition before generating code for it.
    /// This method is called just before the first call to `next_function`.
    fn check_precondition(&mut self, module_info: &ModuleInfo) -> Result<(), CodegenError>;

    /// Creates a new function and returns the function-scope code generator for it.
    fn next_function(&mut self) -> Result<&mut FCG, CodegenError>;
    
    /// Finalizes code generation, returning runtime structures.
    fn finalize(self, module_info: &ModuleInfo) -> Result<(PC, FR), CodegenError>;

    /// Sets signatures.
    fn feed_signatures(&mut self, signatures: Map<SigIndex, FuncSig>) -> Result<(), CodegenError>;

    /// Sets function signatures.
    fn feed_function_signatures(
        &mut self,
        assoc: Map<FuncIndex, SigIndex>,
    ) -> Result<(), CodegenError>;

    /// Adds an import function.
    fn feed_import_function(&mut self) -> Result<(), CodegenError>;
}

/// The function-scope code generator trait.
pub trait FunctionCodeGenerator {
    /// Sets the return type.
    fn feed_return(&mut self, ty: WpType) -> Result<(), CodegenError>;

    /// Adds a parameter to the function.
    fn feed_param(&mut self, ty: WpType) -> Result<(), CodegenError>;

    /// Adds `n` locals to the function.
    fn feed_local(&mut self, ty: WpType, n: usize) -> Result<(), CodegenError>;

    /// Called before the first call to `feed_opcode`.
    fn begin_body(&mut self) -> Result<(), CodegenError>;

    /// Called for each operator.
    fn feed_opcode(&mut self, op: &Operator, module_info: &ModuleInfo) -> Result<(), CodegenError>;

    /// Finalizes the function.
    fn finalize(&mut self) -> Result<(), CodegenError>;
}

#[derive(Debug)]
pub struct CodegenError {
    pub message: &'static str,
}
