//! Runs a .wast WebAssembly test suites
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use wasmer::sys::engine::NativeEngineExt;
use wasmer_wast::Wast as WastSpectest;

use crate::{backend::RuntimeOptions, common::HashAlgorithm};

#[derive(Debug, Parser)]
/// The options for the `wasmer wast` subcommand
pub struct Wast {
    /// Wast file to run
    #[clap(name = "FILE")]
    path: PathBuf,

    #[clap(flatten)]
    rt: RuntimeOptions,

    #[clap(short, long)]
    /// A flag to indicate wast stop at the first error or continue.
    fail_fast: bool,

    /// Hashing algorithm to be used for module hash
    #[clap(long, value_enum)]
    hash_algorithm: Option<HashAlgorithm>,
}

impl Wast {
    /// Runs logic for the `validate` subcommand
    pub fn execute(&self) -> Result<()> {
        self.inner_execute()
            .context(format!("failed to test the wast `{}`", self.path.display()))
    }
    fn inner_execute(&self) -> Result<()> {
        let mut store = self.rt.get_store()?;

        let engine = store.engine_mut();

        let hash_algorithm = self.hash_algorithm.unwrap_or_default().into();
        engine.set_hash_algorithm(Some(hash_algorithm));

        let mut wast = WastSpectest::new_with_spectest(store);
        wast.fail_fast = self.fail_fast;
        wast.run_file(&self.path).with_context(|| "tests failed")?;
        eprintln!("Wast tests succeeded for `{}`.", self.path.display());
        Ok(())
    }
}
