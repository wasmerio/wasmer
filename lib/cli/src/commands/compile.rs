use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use wasmer::{sys::*, *};

use crate::{backend::RuntimeOptions, warning};

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

        let engine = self.rt.get_engine_for_module(&module_contents, &target)?;

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
                    warning!(
                        "the output file has a wrong extension. We recommend using `{}.{}` for the chosen target",
                        &output_filename,
                        &recommended_extension
                    )
                }
            }
            None => {
                warning!(
                    "the output file has no extension. We recommend using `{}.{}` for the chosen target",
                    &output_filename,
                    &recommended_extension
                )
            }
        }
        println!("Compiler: {}", engine.deterministic_id());
        println!("Target: {}", target.triple());

        let compiler_input = prepare_compiler_input(module_contents)?;
        let module = Module::new(&engine, &compiler_input)?;
        module.serialize_to_file(&self.output)?;
        eprintln!(
            "✔ File compiled successfully to `{}`.",
            self.output.display(),
        );

        Ok(())
    }
}

fn prepare_compiler_input(source: Vec<u8>) -> Result<Vec<u8>> {
    // Only raw native dynamic mains select the existing runtime preparation path.
    // Generic/static/v1 and upstream-prepared inputs keep their original artifact ABI.
    if wasixv2_dylink::needs_preparation(&source)? {
        let prepared = wasixv2_dylink::prepare_image(&source)?;
        Ok(wasixv2::instrument(&prepared)?)
    } else {
        Ok(source)
    }
}

#[cfg(all(test, feature = "wat"))]
mod tests {
    use super::*;

    fn wasm(source: &str) -> Vec<u8> {
        wasmer::wat2wasm(source.as_bytes()).unwrap().into_owned()
    }

    fn dynamic(mut source: Vec<u8>) -> Vec<u8> {
        // Existing dylink.0 empty memory-info fixture used by the loader/planner tests.
        source.extend_from_slice(b"\0\x0f\x08dylink.0\x01\x04\0\0\0\0");
        source
    }

    #[test]
    fn compiler_input_preserves_generic_static_v1_and_prepared_bytes() {
        let native = wasm(r#"(module (import "wasixv2" "exit_group" (func (param i32))))"#);
        let prepared =
            wasixv2::instrument(&wasixv2_dylink::prepare_image(&dynamic(native.clone())).unwrap())
                .unwrap();
        for source in [
            wasm("(module (func $init) (start $init))"),
            native,
            dynamic(wasm(
                r#"(module (import "wasi_snapshot_preview1" "proc_exit" (func (param i32))))"#,
            )),
            prepared,
        ] {
            assert_eq!(prepare_compiler_input(source.clone()).unwrap(), source);
        }
    }
}
