//! Generic Artifact abstraction for Wasmer Engines.

mod trampoline;

pub use self::trampoline::get_libcall_trampoline;
#[cfg(feature = "compiler")]
pub use self::trampoline::*;
