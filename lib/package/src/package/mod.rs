//! Load a Wasmer package from disk.
pub(crate) mod manifest;
pub(crate) mod package;
pub(crate) mod strictness;
pub(crate) mod volume;

pub use self::{
    manifest::ManifestError,
    package::{Package, WasmerPackageError},
    strictness::Strictness,
    volume::{fs::*, in_memory::*, WasmerPackageVolume},
};
