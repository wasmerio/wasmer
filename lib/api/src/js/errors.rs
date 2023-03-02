use crate::js::lib::std::string::String;
pub use crate::js::trap::RuntimeError;
#[cfg(feature = "std")]
use thiserror::Error;
use wasmer_types::ImportError;

/// The WebAssembly.LinkError object indicates an error during
/// module instantiation (besides traps from the start function).
///
/// This is based on the [link error][link-error] API.
///
/// [link-error]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/WebAssembly/LinkError
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(Error))]
#[cfg_attr(feature = "std", error("Link error: {0}"))]
pub enum LinkError {
    /// An error occurred when checking the import types.
    #[cfg_attr(feature = "std", error("Error while importing {0:?}.{1:?}: {2}"))]
    Import(String, String, ImportError),

    /// A trap ocurred during linking.
    #[cfg_attr(feature = "std", error("RuntimeError occurred during linking: {0}"))]
    Trap(#[source] RuntimeError),
    /// Insufficient resources available for linking.
    #[cfg_attr(feature = "std", error("Insufficient resources: {0}"))]
    Resource(String),
}
