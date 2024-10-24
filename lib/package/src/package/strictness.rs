use std::{fmt::Debug, path::Path};

use anyhow::Error;

use super::ManifestError;

/// The strictness to use when working with a
/// [`crate::wasmer_package::Package`].
///
/// This can be useful when loading a package that may be edited interactively
/// or if you just want to use a package and don't care if the manifest is
/// invalid.
#[derive(Debug, Default, Copy, Clone, PartialEq)]
pub enum Strictness {
    /// Prefer to lose data rather than error out.
    #[default]
    Lossy,
    /// All package issues should be errors.
    Strict,
}

impl Strictness {
    pub(crate) fn is_strict(self) -> bool {
        matches!(self, Strictness::Strict)
    }

    pub(crate) fn on_error(&self, _path: &Path, error: Error) -> Result<(), Error> {
        match self {
            Strictness::Lossy => Ok(()),
            Strictness::Strict => Err(error),
        }
    }

    pub(crate) fn outside_base_directory(
        &self,
        path: &Path,
        base_dir: &Path,
    ) -> Result<(), ManifestError> {
        match self {
            Strictness::Lossy => todo!(),
            Strictness::Strict => Err(ManifestError::OutsideBaseDirectory {
                path: path.to_path_buf(),
                base_dir: base_dir.to_path_buf(),
            }),
        }
    }

    pub(crate) fn missing_file(&self, path: &Path, base_dir: &Path) -> Result<(), ManifestError> {
        match self {
            Strictness::Lossy => Ok(()),
            Strictness::Strict => Err(ManifestError::MissingFile {
                path: path.to_path_buf(),
                base_dir: base_dir.to_path_buf(),
            }),
        }
    }
}
