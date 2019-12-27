#![deny(
    dead_code,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
#![feature(proc_macro_hygiene)]
#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://avatars3.githubusercontent.com/u/44205449?s=200&v=4")]

#[cfg(not(any(
    all(target_os = "freebsd", target_arch = "x86_64"),
    all(target_os = "freebsd", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "aarch64"),
    all(target_os = "android", target_arch = "x86_64"),
    all(target_os = "android", target_arch = "aarch64"),
)))]
compile_error!("This crate doesn't yet support compiling on operating systems and architectures other than these:
       - FreeBSD and x86_64
       - FreeBSD and AArch64
       - macOS and x86_64
       - Linux and x86_64
       - Linux and AArch64
       - Android and x86_64
       - Android and AArch64");

extern crate dynasmrt;

extern crate serde;

#[macro_use]
extern crate serde_derive;

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
#[cfg(target_arch = "aarch64")]
mod translator_aarch64;

pub use codegen_x64::X64FunctionCode as FunctionCodeGenerator;
pub use codegen_x64::X64ModuleCodeGenerator as ModuleCodeGenerator;

use wasmer_runtime_core::codegen::SimpleStreamingCompilerGen;
pub type SinglePassCompiler = SimpleStreamingCompilerGen<
    codegen_x64::X64ModuleCodeGenerator,
    codegen_x64::X64FunctionCode,
    codegen_x64::X64ExecutionContext,
    codegen_x64::CodegenError,
>;
