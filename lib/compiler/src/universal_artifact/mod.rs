//! Generic Artifact abstraction for Wasmer Engines.

mod artifact;
mod builder;
mod trampoline;

pub use self::artifact::ArtifactBuild;
pub use self::builder::EngineBuilder;
pub use self::trampoline::*;
