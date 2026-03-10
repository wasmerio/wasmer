mod common;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use self::unix::*;

#[cfg(target_family = "windows")]
mod win;
#[cfg(target_family = "windows")]
pub use self::win::*;

#[cfg(not(any(unix, target_family = "windows")))]
compile_error!("Your system is not yet supported for the llvm-based compiler backend");
