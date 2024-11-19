//! This submodule has the concrete definitions for all the available implenters of the WebAssembly
//! types needed to create a runtime.

#[cfg(feature = "sys")]
pub mod sys;

#[cfg(feature = "wamr")]
pub mod wamr;

#[cfg(feature = "v8")]
pub mod v8;

#[derive(Debug, Clone, Copy)]
/// An enumeration over all the supported runtimes.
pub enum Runtime {
    #[cfg(feature = "sys")]
    /// The `sys` runtime.
    Sys,

    #[cfg(feature = "wamr")]
    /// The `wamr` runtime.
    Wamr,

    #[cfg(feature = "v8")]
    /// The `v8` runtime.
    V8,
}
