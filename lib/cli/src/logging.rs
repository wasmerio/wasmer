//! Logging functions for the debug feature.

/// Subroutine to instantiate the loggers
#[cfg(any(feature = "tracing", feature = "debug"))]
pub fn set_up_logging(verbose: u8) -> Result<(), String> {
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{fmt, EnvFilter};

    let fmt_layer = fmt::layer()
        .with_target(false)
        .with_span_events(fmt::format::FmtSpan::CLOSE)
        .with_thread_ids(true)
        .compact();

    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| match verbose {
            1 => EnvFilter::try_new("debug"),
            _ => EnvFilter::try_new("trace"),
        })
        .unwrap();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .init();

    Ok(())
}
