use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Parser;
use wasmer::{is_wasm, Module};
use wasmer_types::target::Target;

use crate::backend::RuntimeOptions;
#[derive(Debug, Parser)]
/// The options for the `wasmer validate` subcommand
pub struct Validate {
    /// File to validate as WebAssembly
    #[clap(name = "FILE")]
    path: PathBuf,

    #[clap(flatten)]
    rt: RuntimeOptions,
}

impl Validate {
    /// Runs logic for the `validate` subcommand
    pub fn execute(&self) -> Result<()> {
        self.inner_execute()
            .context(format!("failed to validate `{}`", self.path.display()))
    }
    fn inner_execute(&self) -> Result<()> {
        let module_contents = std::fs::read(&self.path)?;
        if !is_wasm(&module_contents) {
            bail!("`wasmer validate` only validates WebAssembly files");
        }

        let engine = self
            .rt
            .get_engine_for_module(&module_contents, &Target::default())?;
        Module::validate(&engine, &module_contents)?;
        eprintln!("Validation passed for `{}`.", self.path.display());
        Ok(())
    }
}
