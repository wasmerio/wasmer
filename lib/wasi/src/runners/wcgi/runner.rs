use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context, Error};
use futures::future::AbortHandle;
use http::{Request, Response};
use hyper::Body;
use tower::{make::Shared, ServiceBuilder};
use tower_http::{catch_panic::CatchPanicLayer, cors::CorsLayer, trace::TraceLayer};
use tracing::Span;
use virtual_fs::{FileSystem, WebcVolumeFileSystem};
use wasmer::{Engine, Module, Store};
use wcgi_host::CgiDialect;
use webc::{
    compat::SharedBytes,
    metadata::{
        annotations::{Wasi, Wcgi},
        Command, Manifest,
    },
    Container,
};

use crate::{
    runners::{
        wasi_common::CommonWasiOptions,
        wcgi::handler::{Handler, SharedState},
        CompileModule, MappedDirectory,
    },
    runtime::task_manager::tokio::TokioTaskManager,
    PluggableRuntime, VirtualTaskManager, WasiEnvBuilder,
};

pub struct WcgiRunner {
    program_name: String,
    config: Config,
    compile: Option<Arc<CompileModule>>,
}

// TODO(Michael-F-Bryan): When we rewrite the existing runner infrastructure,
// make the "Runner" trait contain just these two methods.
impl WcgiRunner {
    fn supports(cmd: &Command) -> Result<bool, Error> {
        Ok(cmd
            .runner
            .starts_with(webc::metadata::annotations::WCGI_RUNNER_URI))
    }

    #[tracing::instrument(skip(self, ctx))]
    fn run(&mut self, command_name: &str, ctx: &RunnerContext<'_>) -> Result<(), Error> {
        let wasi: Wasi = ctx
            .command()
            .annotation("wasi")
            .context("Unable to retrieve the WASI metadata")?
            .unwrap_or_else(|| Wasi::new(command_name));

        let module = self
            .load_module(&wasi, ctx)
            .context("Couldn't load the module")?;

        let handler = self.create_handler(module, &wasi, ctx)?;
        let task_manager = Arc::clone(&handler.task_manager);
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

        task_manager
            .block_on(async {
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

impl WcgiRunner {
    pub fn new(program_name: impl Into<String>) -> Self {
        WcgiRunner {
            program_name: program_name.into(),
            config: Config::default(),
            compile: None,
        }
    }

    pub fn config(&mut self) -> &mut Config {
        &mut self.config
    }

    /// Sets the compile function
    pub fn with_compile(
        mut self,
        compile: impl Fn(&Engine, &[u8]) -> Result<Module, Error> + Send + Sync + 'static,
    ) -> Self {
        self.compile = Some(Arc::new(compile));
        self
    }

    fn load_module(&mut self, wasi: &Wasi, ctx: &RunnerContext<'_>) -> Result<Module, Error> {
        let atom_name = &wasi.atom;
        let atom = ctx
            .get_atom(atom_name)
            .with_context(|| format!("Unable to retrieve the \"{atom_name}\" atom"))?;

        let module = ctx.compile(&atom).context("Unable to compile the atom")?;

        Ok(module)
    }

    fn create_handler(
        &self,
        module: Module,
        wasi: &Wasi,
        ctx: &RunnerContext<'_>,
    ) -> Result<Handler, Error> {
        let Wcgi { dialect, .. } = ctx.command().annotation("wcgi")?.unwrap_or_default();

        let dialect = match dialect {
            Some(d) => d.parse().context("Unable to parse the CGI dialect")?,
            None => CgiDialect::Wcgi,
        };

        let shared = SharedState {
            module,
            dialect,
            program_name: self.program_name.clone(),
            setup_builder: Box::new(self.setup_builder(ctx, wasi)),
            callbacks: Arc::clone(&self.config.callbacks),
            task_manager: self
                .config
                .task_manager
                .clone()
                .unwrap_or_else(|| Arc::new(TokioTaskManager::default())),
        };

        Ok(Handler::new(shared))
    }

    fn setup_builder(
        &self,
        ctx: &RunnerContext<'_>,
        wasi: &Wasi,
    ) -> impl Fn(&mut WasiEnvBuilder) -> Result<(), Error> + Send + Sync {
        let container_fs = ctx.container_fs();
        let wasi_common = self.config.wasi.clone();
        let wasi = wasi.clone();
        let tasks = self.config.task_manager.clone();

        move |builder| {
            wasi_common.prepare_webc_env(builder, Arc::clone(&container_fs), &wasi)?;

            if let Some(tasks) = &tasks {
                let rt = PluggableRuntime::new(Arc::clone(tasks));
                builder.set_runtime(Arc::new(rt));
            }

            Ok(())
        }
    }
}

// TODO(Michael-F-Bryan): Pass this to Runner::run() as a "&dyn RunnerContext"
// when we rewrite the "Runner" trait.
struct RunnerContext<'a> {
    container: &'a Container,
    command: &'a Command,
    compile: Option<Arc<CompileModule>>,
    engine: Engine,
    store: Arc<Store>,
}

#[allow(dead_code)]
impl RunnerContext<'_> {
    fn command(&self) -> &Command {
        self.command
    }

    fn manifest(&self) -> &Manifest {
        self.container.manifest()
    }

    fn store(&self) -> &Store {
        &self.store
    }

    fn get_atom(&self, name: &str) -> Option<SharedBytes> {
        self.container.atoms().remove(name)
    }

    fn container_fs(&self) -> Arc<dyn FileSystem> {
        Arc::new(WebcVolumeFileSystem::mount_all(self.container))
    }

    fn compile(&self, wasm: &[u8]) -> Result<Module, Error> {
        let compile = self
            .compile
            .as_deref()
            .unwrap_or(&crate::runners::default_compile);
        compile(&self.engine, wasm)
    }
}

impl crate::runners::Runner for WcgiRunner {
    type Output = ();

    fn can_run_command(&self, _: &str, command: &Command) -> Result<bool, Error> {
        WcgiRunner::supports(command)
    }

    fn run_command(
        &mut self,
        command_name: &str,
        command: &Command,
        container: &Container,
    ) -> Result<Self::Output, Error> {
        let store = self.config.store.clone().unwrap_or_default();

        let ctx = RunnerContext {
            container,
            command,
            engine: store.engine().clone(),
            store,
            compile: self.compile.clone(),
        };

        WcgiRunner::run(self, command_name, &ctx)
    }
}

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct Config {
    task_manager: Option<Arc<dyn VirtualTaskManager>>,
    wasi: CommonWasiOptions,
    addr: SocketAddr,
    #[derivative(Debug = "ignore")]
    callbacks: Arc<dyn Callbacks>,
    store: Option<Arc<Store>>,
}

impl Config {
    pub fn task_manager(&mut self, task_manager: impl VirtualTaskManager) -> &mut Self {
        self.task_manager = Some(Arc::new(task_manager));
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

    pub fn store(&mut self, store: Store) -> &mut Self {
        self.store = Some(Arc::new(store));
        self
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            task_manager: None,
            addr: ([127, 0, 0, 1], 8000).into(),
            wasi: CommonWasiOptions::default(),
            callbacks: Arc::new(NoopCallbacks),
            store: None,
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
