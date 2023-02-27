use anyhow::{Context, Error};
use wasmer::{Engine, Module, Store};
use wasmer_vfs::FileSystem;
use webc::metadata::{Command, Manifest};

use crate::runners::WapmContainer;

pub struct WcgiRunner {}

// TODO(Michael-F-Bryan): When we rewrite the existing runner infrastructure,
// make the "Runner" trait contain just these two methods.
impl WcgiRunner {
    fn supports(cmd: &Command) -> Result<bool, Error> {
        Ok(cmd.runner.starts_with("https://webc.org/runner/wcgi"))
    }

    #[tracing::instrument(skip(self, ctx))]
    fn run_(&self, command_name: &str, ctx: &RunnerContext<'_>) -> Result<(), Error> {
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
        todo!();
    }
}

// TODO(Michael-F-Bryan): Turn this into an object-safe trait when we rewrite
// the "Runner" trait.
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
        todo!();
    }

    fn get_atom(&self, name: &str) -> Option<&[u8]> {
        self.container.get_atom(name)
    }

    fn compile(&self, wasm: &[u8]) -> Result<Module, Error> {
        // TODO: wire this up to wasmer-cache
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
