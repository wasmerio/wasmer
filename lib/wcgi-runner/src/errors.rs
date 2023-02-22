use std::path::PathBuf;

use wasmer_wasi::{FsError, WasiRuntimeError, WasiStateCreationError};
use webc::Version;

/// Various errors that can be returned by the WCGI runner.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("The provided binary is not in a format known by the runner")]
    UnknownFormat,
    #[error(transparent)]
    Hyper(#[from] hyper::Error),
    #[error(transparent)]
    Io(std::io::Error),
    #[error("Unable to read \"{}\"", path.display())]
    File {
        #[source]
        error: std::io::Error,
        path: PathBuf,
    },
    #[error(transparent)]
    Webc(#[from] WebcLoadError),
    #[error("Unable to compile the WebAssembly module")]
    Compile(#[from] wasmer::CompileError),
    #[error("A spawned task didn't run to completion")]
    Join(#[from] tokio::task::JoinError),
    #[error("Unable to automatically infer the program name")]
    ProgramNameRequired,
    #[error("An error occurred while implementing the CGI protocol")]
    Cgi(#[from] wcgi_host::CgiError),
    #[error("Unable to set up the WASI environment")]
    StateCreation(#[from] WasiStateCreationError),
    #[error("Could not mount directory mapping: '{src}:{dst}'")]
    Mount {
        #[source]
        error: FsError,
        src: PathBuf,
        dst: PathBuf,
    },
    #[error("Executing the WASI executable failed")]
    Exec(#[from] WasiRuntimeError),
}

/// Errors that may occur when loading a WCGI program from a WEBC file.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WebcLoadError {
    #[error("The WEBC file doesn't contain a \"{name}\" command")]
    UnknownCommand { name: String },
    #[error("Unable to find the \"{name}\" atom")]
    MissingAtom { name: String },
    #[error("Unable to parse the manifest")]
    Manifest(#[from] serde_cbor::Error),
    #[error("Unable to detect the WEBC version")]
    Detect(#[from] webc::DetectError),
    #[error("Unsupported WEBC version, {_0}")]
    UnsupportedVersion(Version),
    #[error(transparent)]
    V1(#[from] webc::v1::Error),
    #[error(transparent)]
    V2(#[from] webc::v2::read::OwnedReaderError),
    #[error("Unable to infer the command to execute")]
    UnknownEntrypoint,
}
