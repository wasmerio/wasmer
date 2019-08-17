#![deny(
    dead_code,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
#[macro_use]
extern crate wasmer_runtime_core;
// extern crate wasmer_emscripten;

#[macro_use]
pub mod update;
pub mod utils;
pub mod webassembly;
