use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Parser;
use wasmer::{
    sys::{engine::NativeEngineExt, *},
    *,
};

use crate::{backend::RuntimeOptions, common::HashAlgorithm, warning};

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
    rt: RuntimeOptions,

    #[clap(short = 'm')]
    cpu_features: Vec<CpuFeature>,

    /// Hashing algorithm to be used for module hash
    #[clap(long, value_enum)]
    hash_algorithm: Option<HashAlgorithm>,
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

        let module_contents = std::fs::read(&self.path)?;
        if !is_wasm(&module_contents) {
            bail!("`wasmer compile` only compiles WebAssembly files");
        }

        let mut engine = self
            .rt
            .get_engine_for_module(&module_contents, &Target::default())?;

        let hash_algorithm = self.hash_algorithm.unwrap_or_default().into();
        engine.set_hash_algorithm(Some(hash_algorithm));

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
        println!("Compiler: {}", engine.deterministic_id());
        println!("Target: {}", target.triple());

        let module = Module::new(&engine, &module_contents)?;
        module.serialize_to_file(&self.output)?;
        eprintln!(
            "✔ File compiled successfully to `{}`.",
            self.output.display(),
        );

        Ok(())
    }
}
