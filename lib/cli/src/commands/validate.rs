use crate::store::StoreOptions;
use anyhow::{Context, Result};
use std::path::PathBuf;
use structopt::StructOpt;
use wasmer::*;

#[derive(Debug, StructOpt)]
/// The options for the `wasmer validate` subcommand
pub struct Validate {
    /// File to validate as WebAssembly
    #[structopt(name = "FILE", parse(from_os_str))]
    path: PathBuf,

    #[structopt(flatten)]
    store: StoreOptions,
}

impl Validate {
    /// Runs logic for the `validate` subcommand
    pub fn execute(&self) -> Result<()> {
        self.inner_execute()
            .context(format!("failed to validate `{}`", self.path.display()))
    }
    fn inner_execute(&self) -> Result<()> {
        let (store, _engine_type, _compiler_type) = self.store.get_store()?;
        let module_contents = std::fs::read(&self.path)?;
        Module::validate(&store, &module_contents)?;
        eprintln!("Validation passed for `{}`.", self.path.display());
        Ok(())
    }
}
