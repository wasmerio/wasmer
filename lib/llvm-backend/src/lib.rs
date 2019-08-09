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

use std::path::PathBuf;
use structopt::StructOpt;

pub use code::LLVMFunctionCodeGenerator as FunctionCodeGenerator;
pub use code::LLVMModuleCodeGenerator as ModuleCodeGenerator;

use wasmer_runtime_core::codegen::SimpleStreamingCompilerGen;

pub type LLVMCompiler = SimpleStreamingCompilerGen<
    code::LLVMModuleCodeGenerator,
    code::LLVMFunctionCodeGenerator,
    backend::LLVMBackend,
    code::CodegenError,
>;

#[derive(Debug, StructOpt, Clone)]
/// LLVM backend flags.
pub struct CLIOptions {
    /// Emit LLVM IR before optimization pipeline.
    #[structopt(long = "backend-llvm-pre-opt-ir", parse(from_os_str))]
    pre_opt_ir: Option<PathBuf>,

    /// Emit LLVM IR after optimization pipeline.
    #[structopt(long = "backend-llvm-post-opt-ir", parse(from_os_str))]
    post_opt_ir: Option<PathBuf>,

    /// Emit LLVM generated native code object file.
    #[structopt(long = "backend-llvm-object-file", parse(from_os_str))]
    obj_file: Option<PathBuf>,
}

pub static mut GLOBAL_OPTIONS: CLIOptions = CLIOptions {
    pre_opt_ir: None,
    post_opt_ir: None,
    obj_file: None,
};
