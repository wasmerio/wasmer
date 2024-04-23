//! The Wasmer binary lib

#![doc(html_favicon_url = "https://wasmer.io/images/icons/favicon-32x32.png")]
#![doc(html_logo_url = "https://github.com/wasmerio.png?size=200")]
#![deny(
    missing_docs,
    // dead_code,
    nonstandard_style,
    unused_mut,
    // unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
// Allowed because it makes code more readable.
#![allow(clippy::bool_comparison, clippy::match_like_matches_macro)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

#[cfg(all(target_os = "linux", feature = "tun-tap"))]
mod net;

mod commands;
mod common;
#[macro_use]
mod error;
mod c_gen;
mod logging;
mod opts;
mod package_source;
mod store;
mod types;
mod utils;

mod edge_config;

/// Version number of this crate.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Run the Wasmer CLI.
pub fn run_cli() {
    self::commands::WasmerCmd::run();
}
