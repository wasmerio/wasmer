use crate::store::StoreOptions;
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::str::FromStr;
use structopt::StructOpt;
use wasmer_compiler::{CpuFeature, Target, Triple};
use wasmer_types::is_wasm;

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
        let target = Target::new(
            Triple::from_str("x86_64-linux-gnu").unwrap(),
            CpuFeature::SSE2 | CpuFeature::AVX,
        );
        let (engine, _engine_type, _compiler_type) = self.store.get_engine_for_target(target)?;
        let module_contents = std::fs::read(&self.path)?;
        if !is_wasm(&module_contents) {
            bail!("`wasmer validate` only validates WebAssembly files");
        }
        engine.validate(&module_contents)?;
        eprintln!("Validation passed for `{}`.", self.path.display());
        Ok(())
    }
}
