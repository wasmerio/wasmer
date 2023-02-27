use std::{collections::HashMap, convert::Infallible, net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::{Context, Error};
use wasmer::{Engine, Module, Store};
use wasmer_vfs::FileSystem;
use wcgi_host::CgiDialect;
use webc::metadata::{Command, Manifest};

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
        Ok(cmd.runner.starts_with("https://webc.org/runner/wcgi"))
    }

    #[tracing::instrument(skip(self, ctx))]
    fn run_(&mut self, command_name: &str, ctx: &RunnerContext<'_>) -> Result<(), Error> {
        let module = self.load_module(ctx).context("Couldn't load the module")?;

        let handler = self.create_handler(module, ctx)?;
        let task_manager = Arc::clone(&handler.task_manager);

        let make_service = hyper::service::make_service_fn(move |_| {
            let handler = handler.clone();
            async { Ok::<_, Infallible>(handler) }
        });

        let address = self.config.addr;
        tracing::info!(%address, "Starting the server");

        task_manager
            .block_on(async { hyper::Server::bind(&address).serve(make_service).await })
            .context("Unable to start the server")?;

        todo!();
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

    fn load_module(&self, ctx: &RunnerContext<'_>) -> Result<Module, Error> {
        let wasi: webc::metadata::annotations::Wasi = ctx
            .command()
            .annotations
            .get("wasi")
            .cloned()
            .and_then(|v| serde_cbor::value::from_value(v).ok())
            .context("Unable to retrieve the WASI metadata")?;

        let atom_name = &wasi.atom;
        let atom = ctx
            .get_atom(&atom_name)
            .with_context(|| format!("Unable to retrieve the \"{atom_name}\" atom"))?;

        let module = ctx.compile(atom).context("Unable to compile the atom")?;

        Ok(module)
    }

    fn create_handler(&self, module: Module, ctx: &RunnerContext<'_>) -> Result<Handler, Error> {
        let mut env = HashMap::new();

        if self.config.forward_host_env {
            env.extend(std::env::vars());
        }

        env.extend(self.config.env.clone());

        let webc::metadata::annotations::Wcgi { dialect, .. } = ctx
            .command()
            .annotations
            .get("wcgi")
            .cloned()
            .and_then(|v| serde_cbor::value::from_value(v).ok())
            .context("No \"wcgi\" annotations associated with this command")?;

        let dialect = match dialect {
            Some(d) => d.parse().context("Unable to parse the CGI dialect")?,
            None => CgiDialect::Wcgi,
        };

        let handler = Handler {
            program: Arc::clone(&self.program_name),
            env: Arc::new(env),
            args: self.config.args.clone().into(),
            mapped_dirs: self.config.mapped_dirs.clone().into(),
            task_manager: self
                .config
                .task_manager
                .clone()
                .unwrap_or_else(|| Arc::new(TokioTaskManager::default())),
            module,
            dialect,
        };

        Ok(handler)
    }
}

// TODO(Michael-F-Bryan): Pass this to Runner::run() as "&dyn RunnerContext"
// when we rewrite the "Runner" trait.
struct RunnerContext<'a> {
    container: &'a WapmContainer,
    command: &'a Command,
    engine: Engine,
    store: Store,
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
        let store = Store::default();
        let ctx = RunnerContext {
            container,
            command,
            engine: store.engine().clone(),
            store,
        };

        self.run_(command_name, &ctx)
    }
}

#[derive(Debug)]
pub struct Config {
    task_manager: Option<Arc<dyn VirtualTaskManager>>,
    addr: SocketAddr,
    args: Vec<String>,
    env: HashMap<String, String>,
    forward_host_env: bool,
    mapped_dirs: Vec<MappedDirectory>,
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
        }
    }
}
