#![type_length_limit = "5795522"]
//! Definition and parsing of wasm interfaces
//!
//! wasm interfaces ensure wasm modules conform to a specific shape
//! they do this by asserting on the imports and exports of the module.

pub mod interface;
pub mod interface_matcher;
pub mod parser;
#[cfg(feature = "validation")]
pub mod validate;

pub use interface::*;
