
#[cfg(target_family = "unix")]
mod unix;
#[cfg(target_family = "unix")]
pub use self::unix::*;

#[cfg(target_family = "windows")]
compile_error!("windows not yet supported for the llvm-based compiler backend");