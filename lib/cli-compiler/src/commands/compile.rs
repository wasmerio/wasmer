use crate::store::StoreOptions;
use crate::warning;
use anyhow::{Context, Result};
use clap::Parser;
use std::fs;
use std::path::{Path, PathBuf};
use wasmer_compiler::{ArtifactBuild, ArtifactCreate, ModuleEnvironment};
use wasmer_types::entity::PrimaryMap;
use wasmer_types::{
    Architecture, CompileError, CpuFeature, MemoryIndex, MemoryStyle, TableIndex, TableStyle,
    Target, Triple,
};

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
        let (engine_builder, compiler_type) = self.store.get_engine_for_target(target.clone())?;
        let engine = engine_builder.engine();
        let output_filename = self
            .output
            .file_stem()
            .map(|osstr| osstr.to_string_lossy().to_string())
            .unwrap_or_default();
        // `.wasmu` is the default extension for all the triples. It
        // stands for “Wasm Universal”.
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
        let tunables = self.store.get_tunables_for_target(&target)?;

        println!("Compiler: {}", compiler_type.to_string());
        println!("Target: {}", target.triple());

        // compile and save the artifact (without using module from api)
        let path: &Path = self.path.as_ref();
        let wasm_bytes = std::fs::read(path)?;
        let environ = ModuleEnvironment::new();
        let translation = environ.translate(&wasm_bytes).map_err(CompileError::Wasm)?;
        let module = translation.module;
        let memory_styles: PrimaryMap<MemoryIndex, MemoryStyle> = module
            .memories
            .values()
            .map(|memory_type| tunables.memory_style(memory_type))
            .collect();
        let table_styles: PrimaryMap<TableIndex, TableStyle> = module
            .tables
            .values()
            .map(|table_type| tunables.table_style(table_type))
            .collect();
        let artifact = ArtifactBuild::new(
            &mut engine.inner_mut(),
            &wasm_bytes,
            &target,
            memory_styles,
            table_styles,
        )?;
        let serialized = artifact.serialize()?;
        fs::write(output_filename, serialized)?;
        eprintln!(
            "✔ File compiled successfully to `{}`.",
            self.output.display(),
        );

        Ok(())
    }
}
