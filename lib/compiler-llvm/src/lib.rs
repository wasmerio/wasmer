#![deny(
    nonstandard_style,
    /*
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    */
    unreachable_patterns
)]
#![cfg_attr(
    all(not(target_os = "windows"), not(target_arch = "aarch64")),
    //deny(dead_code)
)]
#![cfg_attr(nightly, feature(unwind_attributes))]
#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://avatars3.githubusercontent.com/u/44205449?s=200&v=4")]

mod compiler;
mod config;
mod trampoline;
mod translator;

pub use crate::compiler::LLVMCompiler;
pub use crate::config::{InkwellMemoryBuffer, InkwellModule, LLVMCallbacks, LLVMConfig};
