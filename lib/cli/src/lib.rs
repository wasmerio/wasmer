//! The Wasmer binary lib

#![doc(html_favicon_url = "https://wasmer.io/images/icons/favicon-32x32.png")]
#![doc(html_logo_url = "https://github.com/wasmerio.png?size=200")]
#![deny(
    missing_docs,
    dead_code,
    nonstandard_style,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
// Allowed because it makes code more readable.
#![allow(clippy::bool_comparison, clippy::match_like_matches_macro)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

#[cfg(all(target_os = "linux", feature = "tun-tap"))]
pub mod net;

pub mod commands;
pub mod common;
#[macro_use]
pub mod error;
pub mod c_gen;
pub mod logging;
mod opts;
pub mod package_source;
pub mod store;
pub mod suggestions;
mod types;
pub mod utils;

mod config;

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
