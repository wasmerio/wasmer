use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use bytes::Bytes;
use tokio::runtime::Handle;
use wasmer::Engine;
use wcgi_host::CgiDialect;

use crate::{
    context::Context,
    module_loader::{FileLoader, ModuleLoader, WasmLoader, WebcCommand, WebcLoader, WebcOptions},
    Error, Runner,
};

/// A builder for initializing a [`Runner`].
///
/// # Examples
///
/// The easiest way to use the builder is by giving it a WEBC file where the
/// default entrypoint is a WCGI command.
///
/// ```rust,no_run
/// use wcgi_runner::Runner;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::io::Error>> {
/// let webc = std::fs::read("path/to/server.webc")?;
/// let runner = Runner::builder().build_webc(webc)?;
/// # Ok(())
/// # }
#[derive(Default)]
pub struct Builder {
    args: Vec<String>,
    program: Option<String>,
    dialect: Option<CgiDialect>,
    engine: Option<Engine>,
    env: HashMap<String, String>,
    forward_host_env: bool,
    mapped_dirs: Vec<(String, PathBuf)>,
    tokio_handle: Option<Handle>,
}

impl Builder {
    pub fn new() -> Self {
        Builder::default()
    }

    /// Set the name of the program.
    pub fn program(self, program: impl Into<String>) -> Self {
        Builder {
            program: Some(program.into()),
            ..self
        }
    }

    /// Add an argument to the WASI executable's command-line arguments.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Add multiple arguments to the WASI executable's command-line arguments.
    pub fn args<A, S>(mut self, args: A) -> Self
    where
        A: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(|s| s.into()));
        self
    }

    /// Expose an environment variable to the guest.
    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(name.into(), value.into());
        self
    }

    /// Expose multiple environment variables to the guest.
    pub fn envs<I, K, V>(mut self, variables: I) -> Self
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
    pub fn forward_host_env(self) -> Self {
        Builder {
            forward_host_env: true,
            ..self
        }
    }

    /// Override the CGI dialect.
    pub fn cgi_dialect(self, dialect: CgiDialect) -> Self {
        Builder {
            dialect: Some(dialect),
            ..self
        }
    }

    /// Map `guest_dir` to a directory on the host.
    pub fn map_dir(mut self, guest_dir: impl Into<String>, host_dir: impl Into<PathBuf>) -> Self {
        self.mapped_dirs.push((guest_dir.into(), host_dir.into()));
        self
    }

    /// Map one or more `guest_dir` to a directory on the host.
    pub fn map_dirs<I, G, H>(mut self, mapping: I) -> Self
    where
        I: IntoIterator<Item = (G, H)>,
        G: Into<String>,
        H: Into<PathBuf>,
    {
        for (guest_dir, host_dir) in mapping {
            self.mapped_dirs.push((guest_dir.into(), host_dir.into()));
        }

        self
    }

    /// Pass in a [`Handle`] to a custom tokio runtime.
    pub fn tokio_handle(self, handle: Handle) -> Self {
        Builder {
            tokio_handle: Some(handle),
            ..self
        }
    }

    /// Set the engine used to compile the WebAssembly module.
    pub fn engine(self, engine: Engine) -> Self {
        Builder {
            engine: Some(engine),
            ..self
        }
    }

    /// Create a [`Runner`] that executes a WEBC file.
    ///
    /// If [`Builder::program`] was set, this will look for the command with
    /// that name in the WEBC file. Otherwise, it will fall back to the WEBC
    /// file's default entrypoint.
    ///
    /// This will infer the [`CgiDialect`] from the WEBC file's metadata
    pub fn build_webc(self, webc: impl Into<Bytes>) -> Result<Runner, Error> {
        let webc = webc.into();

        let options = WebcOptions {
            command: match &self.program {
                Some(program) => WebcCommand::Named(program),
                None => WebcCommand::Entrypoint,
            },
            dialect: self.dialect,
        };
        let loader = WebcLoader::new_with_options(&options, webc)?;

        self.build(loader.load_once())
    }

    /// Create a [`Runner`] that executes a WebAssembly module.
    ///
    /// This requires the [`Builder::program`] to have been set.
    ///
    /// Unless otherwise specified (i.e. via [`Builder::cgi_dialect`]), the
    /// WebAssembly binary will be assumed to implement the [`CgiDialect::Wcgi`]
    /// dialect.
    pub fn build_wasm(self, wasm: impl Into<Bytes>) -> Result<Runner, Error> {
        let wasm = wasm.into();
        let program = self.program.clone().ok_or(Error::ProgramNameRequired)?;

        let loader = match self.dialect {
            Some(dialect) => WasmLoader::new_with_dialect(program, wasm, dialect),
            None => WasmLoader::new(program, wasm),
        };

        self.build(loader.load_once())
    }

    /// Create a new [`Runner`] from a particular file and automatically reload
    /// whenever that file changes.
    pub fn watch(self, path: impl Into<PathBuf>) -> Result<Runner, Error> {
        let path = path.into();
        let loader = FileLoader::new(&path).cached(file_has_changed(path));

        self.build(loader)
    }

    fn build(self, loader: impl ModuleLoader + 'static) -> Result<Runner, Error> {
        let Builder {
            args,
            engine,
            mut env,
            forward_host_env,
            mapped_dirs,
            tokio_handle,
            ..
        } = self;

        if forward_host_env {
            env = std::env::vars().chain(env).collect();
        }

        let tokio_handle = tokio_handle.unwrap_or_else(|| {
            Handle::try_current().expect("The builder can only be used inside a Tokio context")
        });

        let ctx = Context {
            tokio_handle,
            engine: engine.unwrap_or_else(|| {
                let store = wasmer::Store::default();
                store.engine().clone()
            }),
            env: Arc::new(env),
            args: args.into(),
            loader: Box::new(loader),
            mapped_dirs,
        };

        Ok(Runner::new(Arc::new(ctx)))
    }
}

fn file_has_changed(path: PathBuf) -> impl Fn() -> bool + Send + Sync + 'static {
    let last_modified: Mutex<Option<std::time::SystemTime>> = Mutex::new(None);

    move || -> bool {
        let modified = match path.metadata().and_then(|m| m.modified()).ok() {
            Some(m) => m,
            None => {
                // we couldn't determine the last modified time so be
                // conservative mark the cache as invalidated.
                return true;
            }
        };

        let mut last_modified = last_modified.lock().expect("Poisoned");

        let invalidated = match *last_modified {
            Some(last) => last != modified,
            None => true,
        };

        *last_modified = Some(modified);
        invalidated
    }
}
