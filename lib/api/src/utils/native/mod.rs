/// Traits and implementations used to convert between native values and WebAssembly ones.
pub(crate) mod convert;
pub use convert::*;

/// Trait to interact with native functions.
pub(crate) mod typed_func;
pub use typed_func::*;
