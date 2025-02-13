//! This submodule has the concrete definitions for all the available implenters of the WebAssembly
//! types needed to create a runtime.

#[cfg(feature = "sys")]
pub mod sys;

#[cfg(feature = "wamr")]
pub mod wamr;

#[cfg(feature = "wasmi")]
pub mod wasmi;

#[cfg(feature = "v8")]
pub mod v8;

#[cfg(feature = "js")]
pub mod js;

#[cfg(feature = "jsc")]
pub mod jsc;

#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
/// An enumeration over all the supported runtimes.
pub enum BackendKind {
    #[cfg(feature = "sys")]
    /// The `sys` runtime.
    Sys,

    #[cfg(feature = "wamr")]
    /// The `wamr` runtime.
    Wamr,

    #[cfg(feature = "wasmi")]
    /// The `wasmi` runtime.
    Wasmi,

    #[cfg(feature = "v8")]
    /// The `v8` runtime.
    V8,

    #[cfg(feature = "js")]
    /// The `js` runtime.
    Js,

    #[cfg(feature = "jsc")]
    /// The `jsc` runtime.
    Jsc,
}
