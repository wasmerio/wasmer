//! Runs a .wast WebAssembly test suites
use crate::store::StoreOptions;
use anyhow::{Context, Result};
use std::path::PathBuf;
use structopt::StructOpt;
use wasmer_wast::Wast as WastSpectest;

#[derive(Debug, StructOpt)]
/// The options for the `wasmer wast` subcommand
pub struct Wast {
    /// Wast file to run
    #[structopt(name = "FILE", parse(from_os_str))]
    path: PathBuf,

    #[structopt(flatten)]
    store: StoreOptions,

    #[structopt(short, long)]
    /// A flag to indicate wast stop at the first error or continue.
    fail_fast: bool,
}

impl Wast {
    /// Runs logic for the `validate` subcommand
    pub fn execute(&self) -> Result<()> {
        self.inner_execute()
            .context(format!("failed to test the wast `{}`", self.path.display()))
    }
    fn inner_execute(&self) -> Result<()> {
        let (store, _engine_name, _compiler_name) = self.store.get_store()?;
        let mut wast = WastSpectest::new_with_spectest(store);
        wast.fail_fast = self.fail_fast;
        wast.run_file(&self.path).with_context(|| "tests failed")?;
        eprintln!("Wast tests succeeded for `{}`.", self.path.display());
        Ok(())
    }
}
