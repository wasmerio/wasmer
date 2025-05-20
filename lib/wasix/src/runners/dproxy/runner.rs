use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context, Error};
use futures::{stream::FuturesUnordered, StreamExt};
use http::Request;
use tower::ServiceBuilder;
use tower_http::{catch_panic::CatchPanicLayer, cors::CorsLayer, trace::TraceLayer};
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
                    .make_span_with(|request: &Request<hyper::body::Incoming>| {
                        tracing::info_span!(
                            "request",
                            method = %request.method(),
                            uri = %request.uri(),
                            status_code = tracing::field::Empty,
                        )
                    })
                    .on_response(super::super::response_tracing::OnResponseTracer),
            )
            .layer(CatchPanicLayer::new())
            .layer(CorsLayer::permissive())
            .service(handler);

        let address = self.config.addr;
        tracing::info!(%address, "Starting the DProxy server");

        runtime
            .task_manager()
            .spawn_and_block_on(async move {
                let (mut shutdown, _abort_handle) =
                    futures::future::abortable(futures::future::pending::<()>());

                let listener = tokio::net::TcpListener::bind(&address).await?;
                let graceful = hyper_util::server::graceful::GracefulShutdown::new();

                let http = hyper::server::conn::http1::Builder::new();

                let mut futs = FuturesUnordered::new();

                loop {
                    tokio::select! {
                        Ok((stream, _addr)) = listener.accept() => {
                            let io = hyper_util::rt::tokio::TokioIo::new(stream);
                            let service = hyper_util::service::TowerToHyperService::new(service.clone());
                            let conn = http.serve_connection(io, service);
                            // watch this connection
                            let fut = graceful.watch(conn);
                            futs.push(async move {
                                if let Err(e) = fut.await {
                                    eprintln!("Error serving connection: {e:?}");
                                }
                            });
                        },

                        _ = futs.next() => {}

                        _ = &mut shutdown => {
                            tracing::info!("Shutting down gracefully");
                            // stop the accept loop
                            break;
                        }
                    }
                }

                Ok::<_, anyhow::Error>(())
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
