//! Logging functions for the debug feature.

use tracing_subscriber::{
    filter::Directive, fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter,
};

/// Subroutine to instantiate the loggers
pub fn set_up_logging(level: log::LevelFilter) {
    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_span_events(fmt::format::FmtSpan::CLOSE)
        .with_ansi(should_emit_colors())
        .with_thread_ids(true)
        .with_writer(std::io::stderr)
        .compact();

    let filter_layer = EnvFilter::builder()
        .with_default_directive(log_directive(level))
        .from_env_lossy();

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

fn log_directive(level: log::LevelFilter) -> Directive {
    let tracing_level = match level {
        log::LevelFilter::Off => tracing::level_filters::LevelFilter::OFF,
        log::LevelFilter::Error => tracing::level_filters::LevelFilter::ERROR,
        log::LevelFilter::Warn => tracing::level_filters::LevelFilter::WARN,
        log::LevelFilter::Info => tracing::level_filters::LevelFilter::INFO,
        log::LevelFilter::Debug => tracing::level_filters::LevelFilter::DEBUG,
        log::LevelFilter::Trace => tracing::level_filters::LevelFilter::TRACE,
    };

    tracing_level.into()
}
