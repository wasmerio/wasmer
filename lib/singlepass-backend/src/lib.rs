#![deny(unused_imports, unused_variables)]
#![feature(proc_macro_hygiene)]

#[cfg(not(any(
    all(target_os = "macos", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64"),
)))]
compile_error!("This crate doesn't yet support compiling on operating systems other than linux and macos and architectures other than x86_64");

extern crate dynasmrt;

#[macro_use]
extern crate dynasm;

#[macro_use]
extern crate lazy_static;

extern crate byteorder;
#[macro_use]
extern crate smallvec;

mod codegen_x64;
mod emitter_x64;
mod machine;
mod protect_unix;

pub use codegen_x64::X64FunctionCode as FunctionCodeGenerator;
pub use codegen_x64::X64ModuleCodeGenerator as ModuleCodeGenerator;

use wasmer_runtime_core::codegen::SimpleStreamingCompilerGen;
pub type SinglePassCompiler = SimpleStreamingCompilerGen<
    codegen_x64::X64ModuleCodeGenerator,
    codegen_x64::X64FunctionCode,
    codegen_x64::X64ExecutionContext,
    codegen_x64::CodegenError,
>;
