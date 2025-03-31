//! The Wasmer Engine.

mod error;
#[cfg(not(target_arch = "wasm32"))]
mod resolver;
#[cfg(not(target_arch = "wasm32"))]
mod trap;
#[cfg(not(target_arch = "wasm32"))]
mod tunables;

#[cfg(not(target_arch = "wasm32"))]
mod artifact;
mod builder;
#[cfg(not(target_arch = "wasm32"))]
mod code_memory;
mod inner;
#[cfg(not(target_arch = "wasm32"))]
mod link;
#[cfg(not(target_arch = "wasm32"))]
mod unwind;

pub use self::error::{InstantiationError, LinkError};
#[cfg(not(target_arch = "wasm32"))]
pub use self::resolver::resolve_imports;
#[cfg(not(target_arch = "wasm32"))]
pub use self::trap::*;
#[cfg(not(target_arch = "wasm32"))]
pub use self::tunables::{BaseTunables, Tunables};

#[cfg(not(target_arch = "wasm32"))]
pub use self::artifact::Artifact;
pub use self::builder::EngineBuilder;
#[cfg(not(target_arch = "wasm32"))]
pub use self::code_memory::CodeMemory;
pub use self::inner::{Engine, EngineInner};
#[cfg(not(target_arch = "wasm32"))]
pub use self::link::link_module;
