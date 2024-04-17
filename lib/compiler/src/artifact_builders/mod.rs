//! Generic Artifact abstraction for Wasmer Engines.

mod artifact_builder;
#[cfg(feature = "compiler")]
mod trampoline;

pub use self::artifact_builder::{ArtifactBuild, ArtifactBuildFromArchive, ModuleFromArchive};
#[cfg(feature = "compiler")]
pub use self::trampoline::*;
