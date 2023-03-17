#[cfg(feature = "js")]
pub use crate::js::errors::{LinkError, RuntimeError};
use thiserror::Error;
#[cfg(feature = "sys")]
pub use wasmer_compiler::{LinkError, RuntimeError};

/// An error while instantiating a module.
///
/// This is not a common WebAssembly error, however
/// we need to differentiate from a `LinkError` (an error
/// that happens while linking, on instantiation), a
/// Trap that occurs when calling the WebAssembly module
/// start function, and an error when initializing the user's
/// host environments.
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(Error))]
pub enum InstantiationError {
    /// A linking ocurred during instantiation.
    #[cfg_attr(feature = "std", error(transparent))]
    Link(LinkError),

    /// A runtime error occured while invoking the start function
    #[cfg_attr(feature = "std", error(transparent))]
    Start(RuntimeError),

    /// The module was compiled with a CPU feature that is not available on
    /// the current host.
    #[cfg_attr(feature = "std", error("missing required CPU features: {0:?}"))]
    CpuFeature(String),

    /// Import from a different [`Store`][super::Store].
    /// This error occurs when an import from a different store is used.
    #[cfg_attr(feature = "std", error("cannot mix imports from different stores"))]
    DifferentStores,

    /// Import from a different Store.
    /// This error occurs when an import from a different store is used.
    #[cfg_attr(feature = "std", error("incorrect OS or architecture"))]
    DifferentArchOS,
}

#[cfg(feature = "core")]
impl std::fmt::Display for InstantiationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "InstantiationError")
    }
}
