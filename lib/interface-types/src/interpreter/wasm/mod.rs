//! An hypothetic WebAssembly runtime, represented as a set of enums,
//! types, and traits —basically this is the part a runtime should
//! take a look to use the `wasmer-interface-types` crate—.

#[cfg(feature = "serde")]
mod serde;

pub mod structures;
pub mod values;
