//! Generic Artifact abstraction for Wasmer Engines.

mod artifact_builder;
mod trampoline;

pub(crate) use self::artifact_builder::ModuleFile;
pub use self::artifact_builder::{ArtifactBuild, ArtifactBuildFromArchive, ModuleFromArchive};
pub use self::trampoline::get_libcall_trampoline;
#[cfg(feature = "compiler")]
pub use self::trampoline::*;
