//! Runs a .wast WebAssembly test suites
use crate::compiler::CompilerOptions;
use anyhow::{Context, Result};
use std::path::PathBuf;
use structopt::StructOpt;
use wasmer_wast::Wast as WastSpectest;

#[derive(Debug, StructOpt)]
/// The options for the `wasmer wast` subcommand
pub struct Wast {
    /// Input file
    #[structopt(parse(from_os_str))]
    path: PathBuf,

    #[structopt(flatten)]
    compiler: CompilerOptions,

    #[structopt(short, long)]
    /// A flag to indicate wast stop at the first error or continue.
    fail_fast: bool,
}

impl Wast {
    /// Runs logic for the `validate` subcommand
    pub fn execute(&self) -> Result<()> {
        let (store, _compiler_name) = self.compiler.get_store()?;
        let mut wast = WastSpectest::new_with_spectest(store);
        wast.fail_fast = self.fail_fast;
        wast.run_file(&self.path)
            .with_context(|| format!("Test file {} was unsuccessful", self.path.display()))
    }
}
