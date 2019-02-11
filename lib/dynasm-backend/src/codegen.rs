use wasmparser::{Operator, Type as WpType};

pub trait ModuleCodeGenerator<FCG: FunctionCodeGenerator> {
    fn next_function(&mut self) -> Result<&mut FCG, CodegenError>;
}

pub trait FunctionCodeGenerator {
    fn feed_param(&mut self, ty: WpType) -> Result<(), CodegenError>;
    fn feed_local(&mut self, ty: WpType, n: usize) -> Result<(), CodegenError>;
    fn feed_opcode(&mut self, op: Operator) -> Result<(), CodegenError>;
    fn finalize(&mut self) -> Result<(), CodegenError>;
}

#[derive(Debug)]
pub struct CodegenError {
    pub message: &'static str,
}
