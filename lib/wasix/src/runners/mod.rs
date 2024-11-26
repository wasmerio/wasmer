mod runner;

#[cfg(feature = "webc_runner_rt_dcgi")]
pub mod dcgi;
#[cfg(feature = "webc_runner_rt_dproxy")]
pub mod dproxy;
pub mod wasi;
mod wasi_common;
#[cfg(feature = "webc_runner_rt_wcgi")]
pub mod wcgi;

#[cfg(any(feature = "webc_runner_rt_wcgi", feature = "webc_runner_rt_dproxy"))]
mod body {
    use http_body_util::{combinators::BoxBody, BodyExt, Full};

    pub type Body = BoxBody<bytes::Bytes, anyhow::Error>;

    pub fn body_from_data(data: impl Into<bytes::Bytes>) -> Body {
        BoxBody::new(Full::new(data.into()).map_err(|_| -> anyhow::Error { unreachable!() }))
    }

    pub fn body_from_stream<S>(s: S) -> Body
    where
        S: futures::stream::Stream<Item = Result<hyper::body::Frame<bytes::Bytes>, anyhow::Error>>
            + Send
            + Sync
            + 'static,
    {
        BoxBody::new(http_body_util::StreamBody::new(s))
    }
}

#[cfg(any(feature = "webc_runner_rt_wcgi", feature = "webc_runner_rt_dproxy"))]
pub use self::body::*;

pub use self::{
    runner::Runner,
    wasi_common::{
        MappedCommand, MappedDirectory, MountedDirectory, MAPPED_CURRENT_DIR_DEFAULT_PATH,
    },
};

// For some reason, providing the same code to on_response() in a lambda
// causes lifetime-related errors, so we use an owned struct instead to
// make *absolutely* sure it's 'static.
#[cfg(any(feature = "webc_runner_rt_wcgi", feature = "webc_runner_rt_dproxy"))]
mod response_tracing {
    use tower_http::trace::OnResponse;

    #[derive(Clone, Copy)]
    pub struct OnResponseTracer;

    impl<B> OnResponse<B> for OnResponseTracer {
        fn on_response(
            self,
            response: &http::Response<B>,
            _latency: std::time::Duration,
            span: &tracing::Span,
        ) {
            span.record("status_code", tracing::field::display(response.status()));
            tracing::info!("response generated")
        }
    }
}
