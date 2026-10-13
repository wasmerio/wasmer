//! Generic Artifact abstraction for Wasmer Engines.

mod artifact_builder;

pub use self::artifact_builder::{ArtifactBuild, ArtifactBuildFromArchive, ModuleFromArchive};
