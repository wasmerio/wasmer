#![deny(unused_imports, unused_variables, unused_unsafe, unreachable_patterns)]

#[cfg(not(target_os = "windows"))]
#[macro_use]
extern crate failure;

#[cfg(not(target_os = "windows"))]
pub mod vfs;
