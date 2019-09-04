#![deny(dead_code, unused_imports, unused_variables, unused_unsafe, unreachable_patterns)]

#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://avatars3.githubusercontent.com/u/44205449?s=200&v=4")]

#[cfg(not(target_os = "windows"))]
#[macro_use]
extern crate failure;

#[cfg(not(target_os = "windows"))]
pub mod vfs;
