//! Generic Artifact abstraction for Wasmer Engines.

mod artifact;
mod engine;
mod trampoline;

pub use self::artifact::UniversalArtifactBuild;
pub use self::engine::EngineBuilder;
pub use self::trampoline::*;
