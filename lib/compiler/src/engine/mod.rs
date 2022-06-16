//! Generic Engine abstraction for Wasmer Engines.

mod artifact;
mod error;
mod export;
mod inner;
mod resolver;
mod trap;
mod tunables;

#[cfg(feature = "translator")]
mod universal;

pub use self::artifact::Artifact;
pub use self::error::{InstantiationError, LinkError};
pub use self::export::{Export, ExportFunction, ExportFunctionMetadata};
pub use self::inner::{Engine, EngineId};
pub use self::resolver::resolve_imports;
pub use self::trap::*;
pub use self::tunables::Tunables;

#[cfg(feature = "translator")]
pub use self::universal::*;
