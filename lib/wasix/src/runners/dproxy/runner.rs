use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context, Error};
use http::{Request, Response};
use hyper::Body;
use tower::{make::Shared, ServiceBuilder};
use tower_http::{catch_panic::CatchPanicLayer, cors::CorsLayer, trace::TraceLayer};
use tracing::Span;
use webc::metadata::Command;

use crate::{
    bin_factory::BinaryPackage,
    runners::wasi::WasiRunner,
    runtime::{task_manager::VirtualTaskManagerExt, DynRuntime},
};

use super::factory::DProxyInstanceFactory;

#[derive(Debug)]
pub struct DProxyRunner {
    config: Config,
    factory: DProxyInstanceFactory,
}

impl DProxyRunner {
    pub fn new(inner: WasiRunner, pkg: &BinaryPackage) -> Self {
        Self {
            config: Config::new(inner, pkg),
            factory: DProxyInstanceFactory::new(),
        }
    }

    pub fn config(&mut self) -> &mut Config {
        &mut self.config
    }
}

/// The base URI used by a [`DProxy`] runner.
pub const DPROXY_RUNNER_URI: &str = "https://webc.org/runner/dproxy";

impl crate::runners::Runner for DProxyRunner {
    fn can_run_command(command: &Command) -> Result<bool, Error> {
        Ok(command.runner.starts_with(DPROXY_RUNNER_URI))
    }

    fn run_command(
        &mut self,
        command_name: &str,
        _pkg: &BinaryPackage,
        runtime: Arc<DynRuntime>,
    ) -> Result<(), Error> {
        // Create the handler that will process the HTTP requests
        let handler = super::handler::Handler::new(
            self.config.clone(),
            command_name.to_string(),
            self.factory.clone(),
            runtime.clone(),
        );

        // We create a HTTP server which will reverse proxy all the requests
        // to the proxy workload
        let service = ServiceBuilder::new()
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(|request: &Request<Body>| {
                        tracing::info_span!(
                            "request",
                            method = %request.method(),
                            uri = %request.uri(),
                            status_code = tracing::field::Empty,
                        )
                    })
                    .on_response(|response: &Response<_>, _latency: Duration, span: &Span| {
                        span.record("status_code", &tracing::field::display(response.status()));
                        tracing::info!("response generated")
                    }),
            )
            .layer(CatchPanicLayer::new())
            .layer(CorsLayer::permissive())
            .service(handler);

        let address = self.config.addr;
        tracing::info!(%address, "Starting the DProxy server");

        runtime
            .task_manager()
            .spawn_and_block_on(async move {
                let (shutdown, _abort_handle) =
                    futures::future::abortable(futures::future::pending::<()>());

                hyper::Server::bind(&address)
                    .serve(Shared::new(service))
                    .with_graceful_shutdown(async {
                        let _ = shutdown.await;
                        tracing::info!("Shutting down gracefully");
                    })
                    .await
            })
            .context("Unable to start the server")??;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub(crate) inner: WasiRunner,
    pub(crate) addr: SocketAddr,
    pub(crate) pkg: BinaryPackage,
    pub(crate) proxy_connect_init_timeout: Duration,
    pub(crate) proxy_connect_nominal_timeout: Duration,
}

impl Config {
    pub fn new(inner: WasiRunner, pkg: &BinaryPackage) -> Self {
        Self {
            inner,
            pkg: pkg.clone(),
            addr: ([127, 0, 0, 1], 8000).into(),
            proxy_connect_init_timeout: Duration::from_secs(30),
            proxy_connect_nominal_timeout: Duration::from_secs(30),
        }
    }

    pub fn addr(&mut self, addr: SocketAddr) -> &mut Self {
        self.addr = addr;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<DProxyRunner>();
        assert_sync::<DProxyRunner>();
    }
}
