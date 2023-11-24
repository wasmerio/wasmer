use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context, Error};
use futures::future::AbortHandle;
use http::{Request, Response};
use hyper::Body;
use tower::{make::Shared, ServiceBuilder};
use tower_http::{catch_panic::CatchPanicLayer, cors::CorsLayer, trace::TraceLayer};
use tracing::Span;
use virtual_fs::RootFileSystemBuilder;
use virtual_fs::TmpFileSystem;
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

#[derive(Debug, Default)]
pub struct WcgiRunner {
    config: Config,
}

impl WcgiRunner {
    pub fn new() -> Self {
        WcgiRunner::default()
    }

    pub fn config(&mut self) -> &mut Config {
        &mut self.config
    }

    #[tracing::instrument(skip_all)]
    fn prepare_handler(
        &mut self,
        command_name: &str,
        pkg: &BinaryPackage,
        runtime: Arc<dyn Runtime + Send + Sync>,
    ) -> Result<Handler, Error> {
        let cmd = pkg
            .get_command(command_name)
            .with_context(|| format!("The package doesn't contain a \"{command_name}\" command"))?;
        let metadata = cmd.metadata();
        let wasi = metadata
            .annotation("wasi")?
            .unwrap_or_else(|| Wasi::new(command_name));

        let module = runtime.load_module_sync(cmd.atom())?;

        let Wcgi { dialect, .. } = metadata.annotation("wcgi")?.unwrap_or_default();
        let dialect = match dialect {
            Some(d) => d.parse().context("Unable to parse the CGI dialect")?,
            None => CgiDialect::Wcgi,
        };

        let wasi_common = self.config.wasi.clone();
        let rt = Arc::clone(&runtime);

        let root_fs = wasi_common
            .fs
            .clone()
            .unwrap_or_else(|| RootFileSystemBuilder::default().build());

        let pkg = pkg.clone();
        let setup_builder = move |builder: &mut WasiEnvBuilder| {
            wasi_common.prepare_webc_env(builder, &wasi, Some(&pkg))?;
            wasi_common.set_filesystem(builder, root_fs.clone())?;
            builder.set_runtime(Arc::clone(&rt));

            Ok(())
        };

        let shared = SharedState {
            module,
            dialect,
            program_name: command_name.to_string(),
            setup_builder: Box::new(setup_builder),
            callbacks: Arc::clone(&self.config.callbacks),
            runtime,
        };

        Ok(Handler::new(shared))
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
        let handler = self.prepare_handler(command_name, pkg, Arc::clone(&runtime))?;
        let callbacks = Arc::clone(&self.config.callbacks);

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
        tracing::info!(%address, "Starting the server");

        runtime
            .task_manager()
            .spawn_and_block_on(async move {
                let (shutdown, abort_handle) =
                    futures::future::abortable(futures::future::pending::<()>());

                callbacks.started(abort_handle);

                hyper::Server::bind(&address)
                    .serve(Shared::new(service))
                    .with_graceful_shutdown(async {
                        let _ = shutdown.await;
                        tracing::info!("Shutting down gracefully");
                    })
                    .await
            })
            .context("Unable to start the server")?;

        Ok(())
    }
}

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct Config {
    wasi: CommonWasiOptions,
    addr: SocketAddr,
    #[derivative(Debug = "ignore")]
    callbacks: Arc<dyn Callbacks>,
}

impl Config {
    /// Builder method to provide a filesystem to the runner
    pub fn with_fs(&mut self, fs: TmpFileSystem) -> &mut Self {
        self.wasi.fs = Some(fs);
        self
    }

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
        self.wasi.env.push((name.into(), value.into()));
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
        self.wasi.mapped_dirs.push(dir);
        self
    }

    pub fn map_directories(
        &mut self,
        mappings: impl IntoIterator<Item = MappedDirectory>,
    ) -> &mut Self {
        self.wasi.mapped_dirs.extend(mappings.into_iter());
        self
    }

    /// Set callbacks that will be triggered at various points in the runner's
    /// lifecycle.
    pub fn callbacks(&mut self, callbacks: impl Callbacks + Send + Sync + 'static) -> &mut Self {
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            addr: ([127, 0, 0, 1], 8000).into(),
            wasi: CommonWasiOptions::default(),
            callbacks: Arc::new(NoopCallbacks),
        }
    }
}

/// Callbacks that are triggered at various points in the lifecycle of a runner
/// and any WebAssembly instances it may start.
pub trait Callbacks: Send + Sync + 'static {
    /// A callback that is called whenever the server starts.
    fn started(&self, _abort: AbortHandle) {}

    /// Data was written to stderr by an instance.
    fn on_stderr(&self, _stderr: &[u8]) {}

    /// Reading from stderr failed.
    fn on_stderr_error(&self, _error: std::io::Error) {}
}

struct NoopCallbacks;

impl Callbacks for NoopCallbacks {}

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
