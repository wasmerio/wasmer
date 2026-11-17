//! Data types, functions and traits for the `v8` backend.

pub(crate) mod bindings;
pub(crate) mod entities;
pub(crate) mod error;
pub(crate) mod utils;
pub(crate) mod vm;

pub use entities::{engine::Engine as V8, *};

#[cfg(feature = "experimental-async")]
pub(crate) mod jspi;

#[cfg(feature = "experimental-async")]
pub(crate) use jspi::enter_sync;

#[cfg(not(feature = "experimental-async"))]
pub(crate) fn enter_sync<T>(store: &mut crate::StoreMut, f: impl FnOnce() -> T) -> T {
    store.parked(f)
}
