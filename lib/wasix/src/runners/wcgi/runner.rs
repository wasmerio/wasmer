use std::{net::SocketAddr, sync::Arc};

use super::super::Body;
use anyhow::{Context, Error};
use futures::{stream::FuturesUnordered, StreamExt};
use http::{Request, Response};
use tower::ServiceBuilder;
use tower_http::{catch_panic::CatchPanicLayer, cors::CorsLayer, trace::TraceLayer};
use wcgi_host::CgiDialect;
use webc::metadata::{
    annotations::{Wasi, Wcgi},
    Command,
};

use crate::{
    bin_factory::BinaryPackage,
    capabilities::Capabilities,
    runners::{
        wasi_common::CommonWasiOptions,
        wcgi::handler::{Handler, SharedState},
        MappedDirectory,
    },
    runtime::task_manager::VirtualTaskManagerExt,
    Runtime, WasiEnvBuilder,
};

use super::Callbacks;

#[derive(Debug)]
pub struct WcgiRunner {
    config: Config,
}

impl WcgiRunner {
    pub fn new<C>(callbacks: C) -> Self
    where
        C: Callbacks,
    {
        Self {
            config: Config::new(callbacks),
        }
    }

    pub fn config(&mut self) -> &mut Config {
        &mut self.config
    }

    #[tracing::instrument(skip_all)]
    pub(crate) fn prepare_handler(
        &mut self,
        command_name: &str,
        pkg: &BinaryPackage,
        propagate_stderr: bool,
        default_dialect: CgiDialect,
        runtime: Arc<dyn Runtime + Send + Sync>,
    ) -> Result<Handler, Error> {
        let cmd = pkg
            .get_command(command_name)
            .with_context(|| format!("The package doesn't contain a \"{command_name}\" command"))?;
        let metadata = cmd.metadata();
        let wasi = metadata
            .annotation("wasi")?
            .unwrap_or_else(|| Wasi::new(command_name));

        let module = runtime.load_module_sync(&cmd.atom())?;

        let Wcgi { dialect, .. } = metadata.annotation("wcgi")?.unwrap_or_default();
        let dialect = match dialect {
            Some(d) => d.parse().context("Unable to parse the CGI dialect")?,
            None => default_dialect,
        };

        let container_fs = Arc::clone(&pkg.webc_fs);

        let wasi_common = self.config.wasi.clone();
        let rt = Arc::clone(&runtime);
        let setup_builder = move |builder: &mut WasiEnvBuilder| {
            wasi_common.prepare_webc_env(builder, Some(Arc::clone(&container_fs)), &wasi, None)?;
            builder.set_runtime(Arc::clone(&rt));
            Ok(())
        };

        let shared = SharedState {
            module,
            module_hash: pkg.hash(),
            dialect,
            propagate_stderr,
            program_name: command_name.to_string(),
            setup_builder: Arc::new(setup_builder),
            callbacks: Arc::clone(&self.config.callbacks),
            runtime,
        };

        Ok(Handler::new(Arc::new(shared)))
    }

    pub(crate) fn run_command_with_handler<S>(
        &mut self,
        handler: S,
        runtime: Arc<dyn Runtime + Send + Sync>,
    ) -> Result<(), Error>
    where
        S: tower::Service<
            Request<hyper::body::Incoming>,
            Response = http::Response<Body>,
            Error = anyhow::Error,
            Future = std::pin::Pin<
                Box<dyn futures::Future<Output = Result<Response<Body>, Error>> + Send>,
            >,
        >,
        S: Clone + Send + Sync + 'static,
    {
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
        tracing::info!(%address, "Starting the server");

        let callbacks = Arc::clone(&self.config.callbacks);
        runtime.task_manager().spawn_and_block_on(async move {
            let (mut shutdown, abort_handle) =
                futures::future::abortable(futures::future::pending::<()>());

            callbacks.started(abort_handle);

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
                        eprintln!("graceful shutdown signal received");
                        // stop the accept loop
                        break;
                    }
                }
            }

            Ok::<_, anyhow::Error>(())
        })??;

        Ok(())
    }
}

impl crate::runners::Runner for WcgiRunner {
    fn can_run_command(command: &Command) -> Result<bool, Error> {
        Ok(command
            .runner
            .starts_with(webc::metadata::annotations::WCGI_RUNNER_URI))
    }

    fn run_command(
        &mut self,
        command_name: &str,
        pkg: &BinaryPackage,
        runtime: Arc<dyn Runtime + Send + Sync>,
    ) -> Result<(), Error> {
        let handler = self.prepare_handler(
            command_name,
            pkg,
            false,
            CgiDialect::Rfc3875,
            Arc::clone(&runtime),
        )?;
        self.run_command_with_handler(handler, runtime)
    }
}

#[derive(Debug)]
pub struct Config {
    pub(crate) wasi: CommonWasiOptions,
    pub(crate) addr: SocketAddr,
    pub(crate) callbacks: Arc<dyn Callbacks>,
}

impl Config {
    pub fn addr(&mut self, addr: SocketAddr) -> &mut Self {
        self.addr = addr;
        self
    }

    /// Add an argument to the WASI executable's command-line arguments.
    pub fn arg(&mut self, arg: impl Into<String>) -> &mut Self {
        self.wasi.args.push(arg.into());
        self
    }

    /// Add multiple arguments to the WASI executable's command-line arguments.
    pub fn args<A, S>(&mut self, args: A) -> &mut Self
    where
        A: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.wasi.args.extend(args.into_iter().map(|s| s.into()));
        self
    }

    /// Expose an environment variable to the guest.
    pub fn env(&mut self, name: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.wasi.env.insert(name.into(), value.into());
        self
    }

    /// Expose multiple environment variables to the guest.
    pub fn envs<I, K, V>(&mut self, variables: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.wasi
            .env
            .extend(variables.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    /// Forward all of the host's environment variables to the guest.
    pub fn forward_host_env(&mut self) -> &mut Self {
        self.wasi.forward_host_env = true;
        self
    }

    pub fn map_directory(&mut self, dir: MappedDirectory) -> &mut Self {
        self.wasi.mounts.push(dir.into());
        self
    }

    pub fn map_directories(
        &mut self,
        mappings: impl IntoIterator<Item = MappedDirectory>,
    ) -> &mut Self {
        for mapping in mappings {
            self.map_directory(mapping);
        }
        self
    }

    /// Set callbacks that will be triggered at various points in the runner's
    /// lifecycle.
    pub fn callbacks(&mut self, callbacks: impl Callbacks + 'static) -> &mut Self {
        self.callbacks = Arc::new(callbacks);
        self
    }

    /// Add a package that should be available to the instance at runtime.
    pub fn inject_package(&mut self, pkg: BinaryPackage) -> &mut Self {
        self.wasi.injected_packages.push(pkg);
        self
    }

    /// Add packages that should be available to the instance at runtime.
    pub fn inject_packages(
        &mut self,
        packages: impl IntoIterator<Item = BinaryPackage>,
    ) -> &mut Self {
        self.wasi.injected_packages.extend(packages);
        self
    }

    pub fn capabilities(&mut self) -> &mut Capabilities {
        &mut self.wasi.capabilities
    }

    #[cfg(feature = "journal")]
    pub fn add_snapshot_trigger(&mut self, on: crate::journal::SnapshotTrigger) {
        self.wasi.snapshot_on.push(on);
    }

    #[cfg(feature = "journal")]
    pub fn add_default_snapshot_triggers(&mut self) -> &mut Self {
        for on in crate::journal::DEFAULT_SNAPSHOT_TRIGGERS {
            if !self.has_snapshot_trigger(on) {
                self.add_snapshot_trigger(on);
            }
        }
        self
    }

    #[cfg(feature = "journal")]
    pub fn has_snapshot_trigger(&self, on: crate::journal::SnapshotTrigger) -> bool {
        self.wasi.snapshot_on.iter().any(|t| *t == on)
    }

    #[cfg(feature = "journal")]
    pub fn with_snapshot_interval(&mut self, period: std::time::Duration) -> &mut Self {
        if !self.has_snapshot_trigger(crate::journal::SnapshotTrigger::PeriodicInterval) {
            self.add_snapshot_trigger(crate::journal::SnapshotTrigger::PeriodicInterval);
        }
        self.wasi.snapshot_interval.replace(period);
        self
    }

    #[cfg(feature = "journal")]
    pub fn add_journal(&mut self, journal: Arc<crate::journal::DynJournal>) -> &mut Self {
        self.wasi.journals.push(journal);
        self
    }
}

impl Config {
    pub fn new<C>(callbacks: C) -> Self
    where
        C: Callbacks,
    {
        Self {
            addr: ([127, 0, 0, 1], 8000).into(),
            wasi: CommonWasiOptions::default(),
            callbacks: Arc::new(callbacks),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<WcgiRunner>();
        assert_sync::<WcgiRunner>();
    }
}
