//! Utilities to set up tracing and logging.
//!
//! By default, wasmer does not generate any tracing output. To
//! enable tracing, one must call the `wasmer_setup_tracing()`
//! function during the initialization stage of their program.
//!
//! Tracing levels can be enabled/disabled using the [RUST_LOG
//! env var](https://docs.rs/env_logger/latest/env_logger/#enabling-logging).
//!
//! # Example
//!
//! ```rust
//! # use wasmer_inline_c::assert_c;
//! # fn main() {
//! #    (assert_c! {
//! # #include "tests/wasmer.h"
//! #
//! int main() {
//!     // This can go up to 4, which is the most verbose
//!     int verbosity_level = 0;
//!     // Whether to use colors when logging information
//!     int use_colors = 1;
//!     wasmer_setup_tracing(verbosity_level, use_colors);
//!
//!     return 0;
//! }
//! #    })
//! #    .success();
//! # }

use std::ffi::c_int;

use tracing::level_filters::LevelFilter;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

const WHITELISTED_LOG_TARGETS: &[&str] = &["wasmer", "wasmer_wasix", "virtual_fs"];

#[no_mangle]
pub extern "C" fn wasmer_setup_tracing(verbosity_level: c_int, use_color: c_int) {
    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_span_events(fmt::format::FmtSpan::CLOSE)
        .with_ansi(use_color > 0)
        .with_thread_ids(true)
        .with_writer(std::io::stderr);

    let filter_layer = {
        let default_filters = [
            LevelFilter::OFF,
            LevelFilter::WARN,
            LevelFilter::INFO,
            LevelFilter::DEBUG,
        ];

        // First, we set up the default log level.
        let default_level = default_filters
            .get(verbosity_level as usize)
            .copied()
            .unwrap_or(LevelFilter::TRACE);
        let mut filter = EnvFilter::builder()
            .with_default_directive(default_level.into())
            .from_env_lossy();

        // Next we add level-specific directives, where verbosity=0 means don't
        // override anything. Note that these are shifted one level up so we'll
        // get something like RUST_LOG="warn,wasmer_wasix=info"
        let specific_filters = [LevelFilter::WARN, LevelFilter::INFO, LevelFilter::DEBUG];
        if verbosity_level > 0 {
            let level = specific_filters
                .get(verbosity_level as usize)
                .copied()
                .unwrap_or(LevelFilter::TRACE);

            for target in WHITELISTED_LOG_TARGETS {
                let directive = format!("{target}={level}").parse().unwrap();
                filter = filter.add_directive(directive);
            }
        }

        filter
    };

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer.compact().with_target(true))
        .init();
}
