#[cfg(feature = "authoring")]
use std::path::PathBuf;

use webc::{ContainerError, DetectError};

#[cfg(feature = "authoring")]
use crate::package::ManifestError;

/// Errors that may occur while loading a Wasmer package.
#[derive(Debug, thiserror::Error)]
#[allow(clippy::result_large_err)]
#[non_exhaustive]
pub enum WasmerPackageError {
    #[cfg(feature = "authoring")]
    #[error("Unable to create a temporary directory")]
    TempDir(#[source] std::io::Error),
    #[cfg(feature = "authoring")]
    #[error("Unable to open \"{}\"", path.display())]
    FileOpen {
        path: PathBuf,
        #[source]
        error: std::io::Error,
    },
    #[cfg(feature = "authoring")]
    #[error("Unable to read \"{}\"", path.display())]
    FileRead {
        path: PathBuf,
        #[source]
        error: std::io::Error,
    },
    #[cfg(feature = "authoring")]
    #[error("IO Error: {0:?}")]
    IoError(#[from] std::io::Error),
    #[cfg(feature = "authoring")]
    #[error("Malformed path format: {0:?}")]
    MalformedPath(PathBuf),
    #[cfg(feature = "authoring")]
    #[error("Unable to extract the tarball")]
    Tarball(#[source] std::io::Error),
    #[cfg(feature = "authoring")]
    #[error("Unable to deserialize \"{}\"", path.display())]
    TomlDeserialize {
        path: PathBuf,
        #[source]
        error: toml::de::Error,
    },
    #[cfg(feature = "authoring")]
    #[error("Unable to deserialize \"{}\"", path.display())]
    JsonDeserialize {
        path: PathBuf,
        #[source]
        error: serde_json::Error,
    },
    #[cfg(feature = "authoring")]
    #[error("Unable to find the \"wasmer.toml\"")]
    MissingManifest,
    #[cfg(feature = "authoring")]
    #[error("Unable to get the absolute path for \"{}\"", path.display())]
    Canonicalize {
        path: PathBuf,
        #[source]
        error: std::io::Error,
    },
    #[cfg(feature = "authoring")]
    #[error("Unable to load the \"wasmer.toml\" manifest")]
    Manifest(#[from] ManifestError),
    #[cfg(feature = "authoring")]
    #[error("The manifest is invalid")]
    Validation(#[from] wasmer_config::package::ValidationError),
    #[cfg(feature = "authoring")]
    #[error("Path: \"{}\" does not exist", path.display())]
    PathNotExists { path: PathBuf },
    #[cfg(feature = "authoring")]
    #[error("Volume creation failed: {0:?}")]
    VolumeCreation(#[from] anyhow::Error),
    #[cfg(feature = "authoring")]
    #[error("serde error: {0:?}")]
    SerdeError(#[from] ciborium::value::Error),
    #[error("container error: {0:?}")]
    ContainerError(#[from] ContainerError),
    #[error("detect error: {0:?}")]
    DetectError(#[from] DetectError),
}
