//! Generic Artifact abstraction for Wasmer Engines.

mod artifact_builder;
mod next_artifact;
mod trampoline;

pub use self::artifact_builder::ArtifactBuild;
pub use self::next_artifact::{CompilationResult, NextArtifact};
pub use self::trampoline::*;
