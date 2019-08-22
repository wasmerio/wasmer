#![deny(
    dead_code,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
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
        builder.set("opt_level", "best").unwrap();
        builder.set("jump_tables_enabled", "false").unwrap();

        if cfg!(not(test)) {
            builder.set("enable_verifier", "false").unwrap();
        }

        let flags = settings::Flags::new(builder);
        debug_assert_eq!(flags.opt_level(), settings::OptLevel::Best);
        flags
    };
    isa::lookup(Triple::host()).unwrap().finish(flags)
}

/// The current version of this crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

use wasmer_runtime_core::codegen::SimpleStreamingCompilerGen;

pub type CraneliftCompiler = SimpleStreamingCompilerGen<
    code::CraneliftModuleCodeGenerator,
    code::CraneliftFunctionCodeGenerator,
    signal::Caller,
    code::CodegenError,
>;
