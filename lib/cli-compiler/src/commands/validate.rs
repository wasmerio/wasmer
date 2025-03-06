use crate::store::StoreOptions;
use anyhow::{bail, Context, Result};
use clap::Parser;
use std::path::PathBuf;
use std::str::FromStr;
use wasmer_types::is_wasm;
use wasmer_types::target::{CpuFeature, Target, Triple};

#[derive(Debug, Parser)]
/// The options for the `wasmer validate` subcommand
pub struct Validate {
    /// File to validate as WebAssembly
    #[clap(name = "FILE")]
    path: PathBuf,

    #[clap(flatten)]
    store: StoreOptions,
}

impl Validate {
    /// Runs logic for the `validate` subcommand
    pub fn execute(&self) -> Result<()> {
        self.inner_execute()
            .context(format!("failed to validate `{}`", self.path.display()))
    }
    fn inner_execute(&self) -> Result<()> {
        let target = Target::new(
            Triple::from_str("x86_64-linux-gnu").unwrap(),
            CpuFeature::SSE2 | CpuFeature::AVX,
        );
        let (engine_builder, _compiler_type) = self.store.get_engine_for_target(target)?;
        let engine = engine_builder.engine();
        let module_contents = std::fs::read(&self.path)?;
        if !is_wasm(&module_contents) {
            bail!("`wasmer validate` only validates WebAssembly files");
        }
        engine.validate(&module_contents)?;
        eprintln!("Validation passed for `{}`.", self.path.display());
        Ok(())
    }
}
