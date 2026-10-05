use webc::{ContainerError, DetectError};

#[cfg(feature = "authoring")]
pub use self::authoring::AuthoringError;

/// Errors that may occur while loading a Wasmer package.
#[derive(Debug, thiserror::Error)]
#[allow(clippy::result_large_err)]
#[non_exhaustive]
pub enum WasmerPackageError {
    /// An error from building or unpacking a package from its sources.
    #[cfg(feature = "authoring")]
    #[error(transparent)]
    Authoring(#[from] AuthoringError),

    /// Container Error
    #[error("container error: {0:?}")]
    ContainerError(#[from] ContainerError),

    /// Detect Error
    #[error("detect error: {0:?}")]
    DetectError(#[from] DetectError),
}

#[cfg(feature = "authoring")]
mod authoring {
    use std::path::PathBuf;

    use crate::package::ManifestError;

    use super::WasmerPackageError;

    /// Errors that may occur while building a package from a directory, a
    /// `wasmer.toml` manifest, or a tarball.
    #[derive(Debug, thiserror::Error)]
    #[non_exhaustive]
    pub enum AuthoringError {
        /// Unable to create a temporary directory.
        #[error("Unable to create a temporary directory")]
        TempDir(#[source] std::io::Error),
        /// Unable to open a file.
        #[error("Unable to open \"{}\"", path.display())]
        FileOpen {
            /// The file being opened.
            path: PathBuf,
            /// The underlying error.
            #[source]
            error: std::io::Error,
        },
        /// Unable to read a file.
        #[error("Unable to read \"{}\"", path.display())]
        FileRead {
            /// The file being opened.
            path: PathBuf,
            /// The underlying error.
            #[source]
            error: std::io::Error,
        },

        /// Generic IO error.
        #[error("IO Error: {0:?}")]
        IoError(#[from] std::io::Error),

        /// Unexpected path format
        #[error("Malformed path format: {0:?}")]
        MalformedPath(PathBuf),

        /// Unable to extract the tarball.
        #[error("Unable to extract the tarball")]
        Tarball(#[source] std::io::Error),
        /// Unable to deserialize the `wasmer.toml` file.
        #[error("Unable to deserialize \"{}\"", path.display())]
        TomlDeserialize {
            /// The file being deserialized.
            path: PathBuf,
            /// The underlying error.
            #[source]
            error: toml::de::Error,
        },
        /// Unable to deserialize a json file.
        #[error("Unable to deserialize \"{}\"", path.display())]
        JsonDeserialize {
            /// The file being deserialized.
            path: PathBuf,
            /// The underlying error.
            #[source]
            error: serde_json::Error,
        },
        /// Unable to find the `wasmer.toml` file.
        #[error("Unable to find the \"wasmer.toml\"")]
        MissingManifest,
        /// Unable to canonicalize a path.
        #[error("Unable to get the absolute path for \"{}\"", path.display())]
        Canonicalize {
            /// The path being canonicalized.
            path: PathBuf,
            /// The underlying error.
            #[source]
            error: std::io::Error,
        },
        /// Unable to load the `wasmer.toml` manifest.
        #[error("Unable to load the \"wasmer.toml\" manifest")]
        Manifest(#[from] ManifestError),
        /// A manifest validation error.
        #[error("The manifest is invalid")]
        Validation(#[from] wasmer_config::package::ValidationError),
        /// A path in the fs mapping does not exist
        #[error("Path: \"{}\" does not exist", path.display())]
        PathNotExists {
            /// Path entry in fs mapping
            path: PathBuf,
        },
        /// Any error happening when populating the volumes tree map of a package
        #[error("Volume creation failed: {0:?}")]
        VolumeCreation(#[from] anyhow::Error),

        /// Error when serializing or deserializing
        #[error("serde error: {0:?}")]
        SerdeError(#[from] ciborium::value::Error),
    }

    // `?` only applies one `From` conversion, so forward the sources that
    // `AuthoringError` absorbs straight into `WasmerPackageError`.
    macro_rules! forward_from {
        ($($source:ty),* $(,)?) => {
            $(
                impl From<$source> for WasmerPackageError {
                    fn from(error: $source) -> Self {
                        WasmerPackageError::Authoring(error.into())
                    }
                }
            )*
        };
    }

    forward_from!(
        std::io::Error,
        ManifestError,
        wasmer_config::package::ValidationError,
        anyhow::Error,
        ciborium::value::Error,
    );
}
