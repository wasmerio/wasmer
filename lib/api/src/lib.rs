#[cfg(all(feature = "sys", feature = "js"))]
compile_error!(
    "Cannot have both `sys` and `js` features enabled at the same time. Please, pick one."
);

#[cfg(all(feature = "sys", target_arch = "wasm32"))]
compile_error!("The `sys` feature must be enabled only for non-`wasm32` target.");

#[cfg(all(feature = "js", not(target_arch = "wasm32")))]
compile_error!(
    "The `js` feature must be enabled only for the `wasm32` target (either `wasm32-unknown-unknown` or `wasm32-wasi`)."
);

#[cfg(feature = "sys")]
mod sys;

#[cfg(feature = "sys")]
pub use sys::*;

#[cfg(feature = "js")]
mod js;

#[cfg(feature = "js")]
pub use js::*;
