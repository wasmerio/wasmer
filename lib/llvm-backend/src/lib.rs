#![deny(
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
#![cfg_attr(not(target_os = "windows"), deny(dead_code))]
#![cfg_attr(nightly, feature(unwind_attributes))]
#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://avatars3.githubusercontent.com/u/44205449?s=200&v=4")]

mod backend;
mod code;
mod intrinsics;
mod platform;
mod read_info;
mod stackmap;
mod state;
mod structs;
mod trampolines;

use std::path::PathBuf;

pub use code::LLVMFunctionCodeGenerator as FunctionCodeGenerator;
pub use code::LLVMModuleCodeGenerator as ModuleCodeGenerator;

use wasmer_runtime_core::codegen::SimpleStreamingCompilerGen;

pub type LLVMCompiler = SimpleStreamingCompilerGen<
    code::LLVMModuleCodeGenerator,
    code::LLVMFunctionCodeGenerator,
    backend::LLVMBackend,
    code::CodegenError,
>;

#[derive(Debug, Clone)]
/// LLVM backend flags.
pub struct LLVMOptions {
    /// Emit LLVM IR before optimization pipeline.
    pub pre_opt_ir: Option<PathBuf>,

    /// Emit LLVM IR after optimization pipeline.
    pub post_opt_ir: Option<PathBuf>,

    /// Emit LLVM generated native code object file.
    pub obj_file: Option<PathBuf>,
}

pub static mut GLOBAL_OPTIONS: LLVMOptions = LLVMOptions {
    pre_opt_ir: None,
    post_opt_ir: None,
    obj_file: None,
};
