//! Logging functions for the debug feature.

use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialize logging based on the `$RUST_LOG` environment variable. Logs will
/// be disabled when `$RUST_LOG` isn't set.
pub fn set_up_logging() {
    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_span_events(fmt::format::FmtSpan::CLOSE)
        .with_ansi(should_emit_colors())
        .with_thread_ids(true)
        .with_writer(std::io::stderr)
        .compact();

    let filter_layer = EnvFilter::builder().from_env_lossy();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .init();
}

/// Check whether we should emit ANSI escape codes for log formatting.
///
/// The `tracing-subscriber` crate doesn't have native support for
/// "--color=always|never|auto", so we implement a poor man's version.
///
/// For more, see https://github.com/tokio-rs/tracing/issues/2388
fn should_emit_colors() -> bool {
    isatty::stderr_isatty() && std::env::var_os("NO_COLOR").is_none()
}
