use crate::compiler::CompilerOptions;
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use structopt::StructOpt;
use wasmer::*;

#[derive(Debug, StructOpt)]
/// The options for the `wasmer compile` subcommand
pub struct Compile {
    /// Input file
    #[structopt(name = "FILE", parse(from_os_str))]
    path: PathBuf,

    /// Output file
    #[structopt(name = "OUTPUT", short = "o", parse(from_os_str))]
    output: PathBuf,

    #[structopt(flatten)]
    compiler: CompilerOptions,
}

impl Compile {
    /// Runs logic for the `compile` subcommand
    pub fn execute(&self) -> Result<()> {
        let (store, _compiler_name) = self.compiler.get_store()?;
        let module =
            Module::from_file(&store, &self.path).with_context(|| "Unable to compile the file")?;
        let serialized = module.serialize()?;
        fs::write(&self.output, serialized)?;
        Ok(())
    }
}
