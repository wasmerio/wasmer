#![deny(unused_mut)]
#![doc(html_favicon_url = "https://wasmer.io/images/icons/favicon-32x32.png")]
#![doc(html_logo_url = "https://github.com/wasmerio.png?size=200")]
#![allow(non_camel_case_types, clippy::identity_op)]

//! Wasmer's WASI types implementation.
//!
//! Those types aim at being used by [the `wasmer-wasi`
//! crate](https://github.com/wasmerio/wasmer/blob/master/lib/wasi).

mod advice;
mod directory;
mod error;
mod event;
mod file;
mod io;
mod signal;
mod subscription;
mod time;
mod versions;

pub use crate::time::*;
pub use advice::*;
pub use directory::*;
pub use error::*;
pub use event::*;
pub use file::*;
pub use io::*;
pub use signal::*;
pub use subscription::*;
pub use versions::*;

pub type __wasi_exitcode_t = u32;

pub type __wasi_userdata_t = u64;
