//! Logging functions for the debug feature.

use tracing_subscriber::{
    filter::Directive, fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter,
};

/// Subroutine to instantiate the loggers
pub fn set_up_logging(level: log::LevelFilter) {
    let fmt_layer = fmt::layer()
        .with_target(false)
        .with_span_events(fmt::format::FmtSpan::CLOSE)
        .with_thread_ids(true)
        .compact();

    let filter_layer = EnvFilter::builder()
        .with_default_directive(log_directive(level))
        .from_env_lossy();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .init();
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
