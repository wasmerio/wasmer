#[macro_use]
extern crate error_chain;
extern crate cranelift_codegen;
extern crate cranelift_entity;
extern crate cranelift_native;
extern crate cranelift_wasm;
extern crate libc;
extern crate region;
extern crate structopt;
extern crate wabt;
extern crate wasmparser;
#[macro_use]
extern crate target_lexicon;
extern crate byteorder;
extern crate console;
extern crate indicatif;
pub extern crate nix; // re-exported for usage in macros
extern crate rayon;
#[cfg(windows)]
extern crate winapi;

#[macro_use]
mod macros;
#[macro_use]
pub mod recovery;
pub mod apis;
pub mod common;
#[cfg(test)]
mod emtests;
pub mod sighandler;
#[cfg(test)]
mod spectests;
pub mod update;
pub mod webassembly;
