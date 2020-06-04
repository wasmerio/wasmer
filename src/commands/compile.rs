use crate::store::StoreOptions;
use crate::warning;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::str::FromStr;
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

    /// Compilation Target triple
    #[structopt(long = "target")]
    target_triple: Option<Triple>,

    #[structopt(flatten)]
    compiler: StoreOptions,

    #[structopt(short = "m", multiple = true)]
    cpu_features: Vec<CpuFeature>,
}

impl Compile {
    /// Runs logic for the `compile` subcommand
    pub fn execute(&self) -> Result<()> {
        self.inner_execute()
            .context(format!("failed to compile `{}`", self.path.display()))
    }
    fn inner_execute(&self) -> Result<()> {
        let target = if let Some(ref target_triple) = self.target_triple {
            let mut features = self
                .cpu_features
                .clone()
                .into_iter()
                .fold(CpuFeature::set(), |a, b| a | b);
            // Cranelift requires SSE2, so we have this "hack" for now to facilitate
            // usage
            features = features | CpuFeature::SSE2;
            Target::new(target_triple.clone(), features)
        } else {
            Target::default()
        };
        let (store, engine_name, compiler_name) =
            self.compiler.get_store_for_target(target.clone())?;
        let output_filename = self
            .output
            .file_stem()
            .map(|osstr| osstr.to_string_lossy().to_string())
            .unwrap_or_default();
        let recommended_extension = match engine_name.as_ref() {
            "native" => {
                // TODO: Match it depending on the `BinaryFormat` instead of the
                // `OperatingSystem`.
                match target.triple().operating_system {
                    OperatingSystem::Darwin
                    | OperatingSystem::Ios
                    | OperatingSystem::MacOSX { .. } => "dylib",
                    _ => "so",
                }
            }
            "jit" => "wjit",
            _ => "?",
        };
        match self.output.extension() {
            Some(ext) => {
                if ext != recommended_extension {
                    warning!("the output file has a wrong extension. We recommend using `{}.{}` for the chosen target", &output_filename, &recommended_extension)
                }
            },
            None => {
                warning!("the output file has no extension. We recommend using `{}.{}` for the chosen target", &output_filename, &recommended_extension)
            }
        }
        println!("Engine: {}", engine_name);
        println!("Compiler: {}", compiler_name);
        println!("Target: {}", target.triple());
        let module = Module::from_file(&store, &self.path)?;
        let _ = module.serialize_to_file(&self.output)?;
        eprintln!(
            "✔ File compiled successfully to `{}`.",
            self.output.display(),
        );
        Ok(())
    }
}
