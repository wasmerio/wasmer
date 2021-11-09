use crate::store::{EngineType, StoreOptions};
use crate::warning;
use anyhow::{Context, Result};
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
    #[structopt(name = "OUTPUT PATH", short = "o", parse(from_os_str))]
    output: PathBuf,

    /// Output path for generated header file
    #[structopt(name = "HEADER PATH", long = "header", parse(from_os_str))]
    header_path: Option<PathBuf>,

    /// Compilation Target triple
    #[structopt(long = "target")]
    target_triple: Option<Triple>,

    #[structopt(flatten)]
    store: StoreOptions,

    #[structopt(short = "m", multiple = true, number_of_values = 1)]
    cpu_features: Vec<CpuFeature>,
}

impl Compile {
    /// Runs logic for the `compile` subcommand
    pub fn execute(&self) -> Result<()> {
        self.inner_execute()
            .context(format!("failed to compile `{}`", self.path.display()))
    }

    pub(crate) fn get_recommend_extension(
        engine_type: &EngineType,
        target_triple: &Triple,
    ) -> Result<&'static str> {
        Ok(match engine_type {
            #[cfg(feature = "dylib")]
            EngineType::Dylib => {
                wasmer_engine_dylib::DylibArtifact::get_default_extension(target_triple)
            }
            #[cfg(feature = "universal")]
            EngineType::Universal => {
                wasmer_engine_universal::UniversalArtifact::get_default_extension(target_triple)
            }
            #[cfg(feature = "staticlib")]
            EngineType::Staticlib => {
                wasmer_engine_staticlib::StaticlibArtifact::get_default_extension(target_triple)
            }
            #[cfg(not(all(feature = "dylib", feature = "universal", feature = "staticlib")))]
            _ => bail!("selected engine type is not compiled in"),
        })
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
                features |= CpuFeature::SSE2;
                Target::new(target_triple.clone(), features)
            })
            .unwrap_or_default();
        let (store, engine_type, compiler_type) =
            self.store.get_store_for_target(target.clone())?;
        let output_filename = self
            .output
            .file_stem()
            .map(|osstr| osstr.to_string_lossy().to_string())
            .unwrap_or_default();
        let recommended_extension = Self::get_recommend_extension(&engine_type, target.triple())?;
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
        println!("Engine: {}", engine_type.to_string());
        println!("Compiler: {}", compiler_type.to_string());
        println!("Target: {}", target.triple());

        let module = Module::from_file(&store, &self.path)?;
        let _ = module.serialize_to_file(&self.output)?;
        eprintln!(
            "✔ File compiled successfully to `{}`.",
            self.output.display(),
        );

        #[cfg(feature = "staticlib")]
        if engine_type == EngineType::Staticlib {
            let artifact: &wasmer_engine_staticlib::StaticlibArtifact =
                module.artifact().as_ref().downcast_ref().context("Engine type is Staticlib but could not downcast artifact into StaticlibArtifact")?;
            let symbol_registry = artifact.symbol_registry();
            let metadata_length = artifact.metadata_length();
            let module_info = module.info();
            let header_file_src = crate::c_gen::staticlib_header::generate_header_file(
                module_info,
                symbol_registry,
                metadata_length,
            );

            let header_path = self.header_path.as_ref().cloned().unwrap_or_else(|| {
                let mut hp = PathBuf::from(
                    self.path
                        .file_stem()
                        .map(|fs| fs.to_string_lossy().to_string())
                        .unwrap_or_else(|| "wasm_out".to_string()),
                );
                hp.set_extension("h");
                hp
            });
            // for C code
            let mut header = std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&header_path)?;

            use std::io::Write;
            header.write_all(header_file_src.as_bytes())?;
            eprintln!(
                "✔ Header file generated successfully at `{}`.",
                header_path.display(),
            );
        }
        Ok(())
    }
}
