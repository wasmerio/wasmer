//! Generic Artifact abstraction for Wasmer Engines.

mod artifact;
mod engine;
mod serialize;
mod trampoline;

pub use self::artifact::UniversalArtifactBuild;
pub use self::engine::UniversalEngineBuilder;
pub use self::serialize::SerializableModule;
pub use self::trampoline::*;
