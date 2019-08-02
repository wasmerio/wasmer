#![deny(
    dead_code,
    unused_imports,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
#![cfg_attr(nightly, feature(unwind_attributes))]

mod backend;
mod code;
mod intrinsics;
mod platform;
mod read_info;
mod state;
mod structs;
mod trampolines;

pub use code::LLVMFunctionCodeGenerator as FunctionCodeGenerator;
pub use code::LLVMModuleCodeGenerator as ModuleCodeGenerator;

use wasmer_runtime_core::codegen::SimpleStreamingCompilerGen;

pub type LLVMCompiler = SimpleStreamingCompilerGen<
    code::LLVMModuleCodeGenerator,
    code::LLVMFunctionCodeGenerator,
    backend::LLVMBackend,
    code::CodegenError,
>;
