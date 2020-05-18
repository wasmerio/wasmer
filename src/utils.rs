//! Utility functions for the WebAssembly module
use std::env;

/// Whether or not Wasmer should print with color
pub fn wasmer_should_print_color() -> bool {
    env::var("WASMER_COLOR")
        .ok()
        .and_then(|inner| inner.parse::<bool>().ok())
        .unwrap_or_else(|| atty::is(atty::Stream::Stdout))
}
