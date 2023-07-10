//! The Wasmer binary lib

#![deny(
    missing_docs,
    dead_code,
    nonstandard_style,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
#![doc(html_favicon_url = "https://wasmer.io/images/icons/favicon-32x32.png")]
#![doc(html_logo_url = "https://github.com/wasmerio.png?size=200")]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

#[macro_use]
extern crate anyhow;

pub mod commands;
pub mod common;
#[macro_use]
pub mod error;
pub mod cli;
#[cfg(feature = "debug")]
pub mod logging;
pub mod store;
pub mod utils;

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
