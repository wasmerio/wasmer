use std::{collections::HashMap, convert::Infallible, net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::{Context, Error};
use futures::future::AbortHandle;
use wasmer::{Engine, Module, Store};
use wasmer_vfs::FileSystem;
use wcgi_host::CgiDialect;
use webc::metadata::{
    annotations::{Wasi, Wcgi},
    Command, Manifest,
};

use crate::{
    runners::{
        wcgi::{handler::Handler, MappedDirectory},
        WapmContainer,
    },
    runtime::task_manager::tokio::TokioTaskManager,
    VirtualTaskManager,
};

pub struct WcgiRunner {
    program_name: Arc<str>,
    config: Config,
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
    fn run_(&mut self, command_name: &str, ctx: &RunnerContext<'_>) -> Result<(), Error> {
        let wasi: Wasi = ctx
            .command()
            .get_annotation("wasi")
            .context("Unable to retrieve the WASI metadata")?
            .unwrap_or_else(|| Wasi::new(command_name));

        let module = self
            .load_module(&wasi, ctx)
            .context("Couldn't load the module")?;

        let handler = self.create_handler(module, &wasi, ctx)?;
        let task_manager = Arc::clone(&handler.task_manager);

        let make_service = hyper::service::make_service_fn(move |_| {
            let handler = handler.clone();
            async { Ok::<_, Infallible>(handler) }
        });

        let address = self.config.addr;
        tracing::info!(%address, "Starting the server");

        let callbacks = Arc::clone(&self.config.callbacks);

        task_manager
            .block_on(async {
                let (shutdown, abort_handle) =
                    futures::future::abortable(futures::future::pending::<()>());

                callbacks.started(abort_handle);

                hyper::Server::bind(&address)
                    .serve(make_service)
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
    pub fn new(program_name: impl Into<Arc<str>>) -> Self {
        WcgiRunner {
            program_name: program_name.into(),
            config: Config::default(),
        }
    }

    pub fn config(&mut self) -> &mut Config {
        &mut self.config
    }

    fn load_module(&self, wasi: &Wasi, ctx: &RunnerContext<'_>) -> Result<Module, Error> {
        let atom_name = &wasi.atom;
        let atom = ctx
            .get_atom(&atom_name)
            .with_context(|| format!("Unable to retrieve the \"{atom_name}\" atom"))?;

        let module = ctx.compile(atom).context("Unable to compile the atom")?;

        Ok(module)
    }

    fn create_handler(
        &self,
        module: Module,
        wasi: &Wasi,
        ctx: &RunnerContext<'_>,
    ) -> Result<Handler, Error> {
        let env = construct_env(wasi, self.config.forward_host_env, &self.config.env);
        let args = construct_args(wasi, &self.config.args);

        let Wcgi { dialect, .. } = ctx.command().get_annotation("wcgi")?.unwrap_or_default();

        let dialect = match dialect {
            Some(d) => d.parse().context("Unable to parse the CGI dialect")?,
            None => CgiDialect::Wcgi,
        };

        let handler = Handler {
            program: Arc::clone(&self.program_name),
            env: Arc::new(env),
            args,
            mapped_dirs: self.config.mapped_dirs.clone().into(),
            task_manager: self
                .config
                .task_manager
                .clone()
                .unwrap_or_else(|| Arc::new(TokioTaskManager::default())),
            module,
            dialect,
            callbacks: Arc::clone(&self.config.callbacks),
        };

        Ok(handler)
    }
}

fn construct_args(wasi: &Wasi, extras: &[String]) -> Arc<[String]> {
    let mut args = Vec::new();

    if let Some(main_args) = &wasi.main_args {
        args.extend(main_args.iter().cloned());
    }

    args.extend(extras.iter().cloned());

    args.into()
}

fn construct_env(
    wasi: &Wasi,
    forward_host_env: bool,
    overrides: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = HashMap::new();

    for item in wasi.env.as_deref().unwrap_or_default() {
        // TODO(Michael-F-Bryan): Convert "wasi.env" in the webc crate from an
        // Option<Vec<String>> to a HashMap<String, String> so we avoid this
        // string.split() business
        match item.split_once('=') {
            Some((k, v)) => {
                env.insert(k.to_string(), v.to_string());
            }
            None => {
                env.insert(item.to_string(), String::new());
            }
        }
    }

    if forward_host_env {
        env.extend(std::env::vars());
    }

    env.extend(overrides.clone());

    env
}

// TODO(Michael-F-Bryan): Pass this to Runner::run() as "&dyn RunnerContext"
// when we rewrite the "Runner" trait.
struct RunnerContext<'a> {
    container: &'a WapmContainer,
    command: &'a Command,
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

    fn engine(&self) -> &Engine {
        &self.engine
    }

    fn store(&self) -> &Store {
        &self.store
    }

    fn volume(&self, _name: &str) -> Option<Box<dyn FileSystem>> {
        todo!("Implement a read-only filesystem backed by a volume");
    }

    fn get_atom(&self, name: &str) -> Option<&[u8]> {
        self.container.get_atom(name)
    }

    fn compile(&self, wasm: &[u8]) -> Result<Module, Error> {
        // TODO(Michael-F-Bryan): wire this up to wasmer-cache
        Module::new(&self.engine, wasm).map_err(Error::from)
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
        container: &WapmContainer,
    ) -> Result<Self::Output, Error> {
        let store = self.config.store.clone().unwrap_or_default();

        let ctx = RunnerContext {
            container,
            command,
            engine: store.engine().clone(),
            store,
        };

        self.run_(command_name, &ctx)
    }
}

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct Config {
    task_manager: Option<Arc<dyn VirtualTaskManager>>,
    addr: SocketAddr,
    args: Vec<String>,
    env: HashMap<String, String>,
    forward_host_env: bool,
    mapped_dirs: Vec<MappedDirectory>,
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
        self.args.push(arg.into());
        self
    }

    /// Add multiple arguments to the WASI executable's command-line arguments.
    pub fn args<A, S>(&mut self, args: A) -> &mut Self
    where
        A: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(|s| s.into()));
        self
    }

    /// Expose an environment variable to the guest.
    pub fn env(&mut self, name: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.env.insert(name.into(), value.into());
        self
    }

    /// Expose multiple environment variables to the guest.
    pub fn envs<I, K, V>(&mut self, variables: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.env
            .extend(variables.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    /// Forward all of the host's environment variables to the guest.
    pub fn forward_host_env(&mut self) -> &mut Self {
        self.forward_host_env = true;
        self
    }

    pub fn map_directory(
        &mut self,
        host: impl Into<PathBuf>,
        guest: impl Into<String>,
    ) -> &mut Self {
        self.mapped_dirs.push(MappedDirectory {
            host: host.into(),
            guest: guest.into(),
        });
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
            env: HashMap::new(),
            forward_host_env: false,
            mapped_dirs: Vec::new(),
            args: Vec::new(),
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
