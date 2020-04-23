use crate::compiler::CompilerOptions;
use anyhow::{Context, Result};
use std::path::PathBuf;
use structopt::StructOpt;
use wasmer::*;

#[derive(Debug, StructOpt)]
/// The options for the `wasmer validate` subcommand
pub struct Validate {
    /// Input file
    #[structopt(parse(from_os_str))]
    path: PathBuf,

    #[structopt(flatten)]
    compiler: CompilerOptions,
}

impl Validate {
    /// Runs logic for the `validate` subcommand
    pub fn execute(&self) -> Result<()> {
        let compiler_config = self.compiler.get_config()?;
        let engine = Engine::new(&*compiler_config);
        let store = Store::new(&engine);
        let module_contents = std::fs::read(&self.path)?;
        Module::validate(&store, &module_contents)
            .with_context(|| "Unable to validate the file")?;
        Ok(())
    }
}
