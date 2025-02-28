//! Data types, functions and traits for the `wamr` backend.

pub(crate) mod bindings;
pub(crate) mod entities;
pub(crate) mod error;
pub(crate) mod utils;
pub(crate) mod vm;

pub use entities::{engine::Engine as Wamr, *};
