#![doc(html_favicon_url = "https://wasmer.io/images/icons/favicon-32x32.png")]
#![doc(html_logo_url = "https://github.com/wasmerio.png?size=200")]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(target_os = "windows")]
compile_error!(
    "The LLVM compiler backend is not supported on Windows. Use the V8 backend instead."
);

mod abi;
mod compiler;
mod config;
mod error;
mod object_file;
mod translator;

pub use crate::compiler::LLVMCompiler;
pub use crate::config::{InkwellMemoryBuffer, InkwellModule, LLVM, LLVMCallbacks, LLVMOptLevel};
