#![deny(unused_imports, unused_variables)]

#[cfg(windows)]
mod exception_handling;

#[cfg(windows)]
pub use self::exception_handling::*;
