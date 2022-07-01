//! Generic Artifact abstraction for Wasmer Engines.

mod artifact;
mod engine;
mod trampoline;

pub use self::artifact::UniversalArtifactBuild;
pub use self::engine::UniversalEngineBuilder;
pub use self::trampoline::*;
