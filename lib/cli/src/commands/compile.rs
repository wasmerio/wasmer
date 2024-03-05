use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use wasmer::*;

use crate::{store::StoreOptions, warning};

#[derive(Debug, Parser)]
/// The options for the `wasmer compile` subcommand
pub struct Compile {
    /// Input file
    #[clap(name = "FILE")]
    path: PathBuf,

    /// Output file
    #[clap(name = "OUTPUT PATH", short = 'o')]
    output: PathBuf,

    /// Compilation Target triple
    #[clap(long = "target")]
    target_triple: Option<Triple>,

    #[clap(flatten)]
    store: StoreOptions,

    #[clap(short = 'm')]
    cpu_features: Vec<CpuFeature>,
}

impl Compile {
    /// Runs logic for the `compile` subcommand
    pub fn execute(&self) -> Result<()> {
        self.inner_execute()
            .context(format!("failed to compile `{}`", self.path.display()))
    }

    fn inner_execute(&self) -> Result<()> {
        let target = self
            .target_triple
            .as_ref()
            .map(|target_triple| {
                let mut features = self
                    .cpu_features
                    .clone()
                    .into_iter()
                    .fold(CpuFeature::set(), |a, b| a | b);
                // Cranelift requires SSE2, so we have this "hack" for now to facilitate
                // usage
                if target_triple.architecture == Architecture::X86_64 {
                    features |= CpuFeature::SSE2;
                }
                Target::new(target_triple.clone(), features)
            })
            .unwrap_or_default();
        let (store, compiler_type) = self.store.get_store_for_target(target.clone())?;
        let output_filename = self
            .output
            .file_stem()
            .map(|osstr| osstr.to_string_lossy().to_string())
            .unwrap_or_default();
        // wasmu stands for "WASM Universal"
        let recommended_extension = "wasmu";
        match self.output.extension() {
            Some(ext) => {
                if ext != recommended_extension {
                    warning!("the output file has a wrong extension. We recommend using `{}.{}` for the chosen target", &output_filename, &recommended_extension)
                }
            }
            None => {
                warning!("the output file has no extension. We recommend using `{}.{}` for the chosen target", &output_filename, &recommended_extension)
            }
        }
        println!("Compiler: {}", compiler_type.to_string());
        println!("Target: {}", target.triple());

        let module = Module::from_file(&store, &self.path)?;
        module.serialize_to_file(&self.output)?;
        eprintln!(
            "✔ File compiled successfully to `{}`.",
            self.output.display(),
        );

        Ok(())
    }
}
