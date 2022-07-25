//! The Wasmer Engine.

mod error;
mod resolver;
mod trap;
mod tunables;

#[cfg(feature = "translator")]
mod artifact;
#[cfg(feature = "translator")]
mod backend;
#[cfg(feature = "translator")]
mod code_memory;
#[cfg(feature = "translator")]
mod inner;
#[cfg(feature = "translator")]
mod link;
#[cfg(feature = "translator")]
mod unwind;

pub use self::error::{InstantiationError, LinkError};
pub use self::resolver::resolve_imports;
pub use self::trap::*;
pub use self::tunables::Tunables;

#[cfg(feature = "translator")]
pub use self::artifact::Artifact;
#[cfg(feature = "translator")]
pub use self::backend::Backend;
#[cfg(feature = "translator")]
pub use self::code_memory::CodeMemory;
#[cfg(feature = "translator")]
pub use self::inner::Engine;
#[cfg(feature = "translator")]
pub use self::link::link_module;
