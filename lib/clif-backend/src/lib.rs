//! The Wasmer Cranelift Backend crate is used to compile wasm binary code via parse events from the
//! Wasmer runtime common parser code into machine code.
//!

#![deny(
    dead_code,
    missing_docs,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://avatars3.githubusercontent.com/u/44205449?s=200&v=4")]

mod cache;
mod code;
mod libcalls;
mod module;
mod relocation;
mod resolver;
mod signal;
mod trampoline;

use cranelift_codegen::{
    isa,
    settings::{self, Configurable},
};
use target_lexicon::Triple;

#[macro_use]
extern crate serde_derive;

extern crate rayon;
extern crate serde;

fn get_isa() -> Box<dyn isa::TargetIsa> {
    let flags = {
        let mut builder = settings::builder();
        builder.set("opt_level", "speed_and_size").unwrap();
        builder.set("jump_tables_enabled", "false").unwrap();

        if cfg!(not(test)) {
            builder.set("enable_verifier", "false").unwrap();
        }

        let flags = settings::Flags::new(builder);
        debug_assert_eq!(flags.opt_level(), settings::OptLevel::SpeedAndSize);
        flags
    };
    isa::lookup(Triple::host()).unwrap().finish(flags)
}

/// The current version of this crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

use wasmer_runtime_core::codegen::SimpleStreamingCompilerGen;

/// Streaming compiler implementation for the Cranelift backed. Compiles web assembly binary into
/// machine code.
pub type CraneliftCompiler = SimpleStreamingCompilerGen<
    code::CraneliftModuleCodeGenerator,
    code::CraneliftFunctionCodeGenerator,
    signal::Caller,
    code::CodegenError,
>;
