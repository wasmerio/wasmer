#[cfg(feature = "link_external_libs")]
#[path = "link-ext.rs"]
pub mod link_ext;
#[doc(inline)]
#[cfg(feature = "link_external_libs")]
pub use crate::link_ext::*;

#[cfg(not(feature = "link_external_libs"))]
use wasmer_wasi::{WasiFs, WasiInodes};
#[cfg(not(feature = "link_external_libs"))]
pub fn initialize(_: &mut WasiInodes, _: &mut WasiFs) -> Result<(), String> {
    Err("wasi-experimental-io-devices has to be compiled with --features=\"link_external_libs\" (not enabled by default) for graphics I/O to work".to_string())
}
