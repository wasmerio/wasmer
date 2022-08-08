#![allow(dead_code)]
//! Create a standalone native executable for a given Wasm file.

use super::ObjectFormat;
use crate::store::CompilerOptions;
use anyhow::{Context, Result};
use clap::Parser;
use std::env;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufWriter;
use std::path::PathBuf;
use std::process::Command;
use wasmer::*;
use wasmer_object::{emit_serialized, get_object_for_target};

const WASMER_SERIALIZED_HEADER: &[u8] = include_bytes!("wasmer_create_exe.h");

#[cfg(feature = "static-artifact-create")]
pub type PrefixerFn = Box<dyn Fn(&[u8]) -> String + Send>;

#[derive(Debug, Parser)]
/// The options for the `wasmer create-exe` subcommand
pub struct CreateObj {
    /// Input file
    #[clap(name = "FILE", parse(from_os_str))]
    path: PathBuf,

    /// Output file
    #[clap(name = "OUTPUT_PATH", short = 'o', parse(from_os_str))]
    output: PathBuf,

    /// Header output file
    #[clap(
        name = "OUTPUT_HEADER_PATH",
        long = "output-header-path",
        parse(from_os_str)
    )]
    header_output: Option<PathBuf>,

    /// Compilation Target triple
    #[clap(long = "target")]
    target_triple: Option<Triple>,

    /// Object format options
    ///
    /// This flag accepts two options: `symbols` or `serialized`.
    /// - (default) `symbols` creates an object where all functions and metadata of the module are regular object symbols
    /// - `serialized` creates an object where the module is zero-copy serialized as raw data
    #[clap(name = "OBJECT_FORMAT", long = "object-format", verbatim_doc_comment)]
    object_format: Option<ObjectFormat>,

    #[clap(short = 'm', multiple = true, number_of_values = 1)]
    cpu_features: Vec<CpuFeature>,

    #[clap(flatten)]
    compiler: CompilerOptions,
}

impl CreateObj {
    /// Runs logic for the `create-obj` subcommand
    pub fn execute(&self) -> Result<()> {
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
        let (store, compiler_type) = self.compiler.get_store_for_target(target.clone())?;
        let object_format = self.object_format.unwrap_or(ObjectFormat::Symbols);

        println!("Compiler: {}", compiler_type.to_string());
        println!("Target: {}", target.triple());
        println!("Format: {:?}", object_format);

        let starting_cd = env::current_dir()?;

        let output_path = starting_cd.join(&self.output);
        let header_output = self.header_output.clone().unwrap_or_else(|| {
            let mut retval = self.output.clone();
            retval.set_extension("h");
            retval
        });

        let header_output_path = starting_cd.join(&header_output);
        let wasm_module_path = starting_cd.join(&self.path);

        match object_format {
            ObjectFormat::Serialized => {
                let module = Module::from_file(&store, &wasm_module_path)
                    .context("failed to compile Wasm")?;
                let bytes = module.serialize()?;
                let mut obj = get_object_for_target(target.triple())?;
                emit_serialized(&mut obj, &bytes, target.triple(), "WASMER_MODULE")?;
                let mut writer = BufWriter::new(File::create(&output_path)?);
                obj.write_stream(&mut writer)
                    .map_err(|err| anyhow::anyhow!(err.to_string()))?;
                writer.flush()?;
                let mut writer = BufWriter::new(File::create(&header_output_path)?);
                writer.write_all(WASMER_SERIALIZED_HEADER)?;
                writer.flush()?;
            }
            ObjectFormat::Symbols => {
                let engine = store.engine();
                let engine_inner = engine.inner();
                let compiler = engine_inner.compiler()?;
                let features = engine_inner.features();
                let tunables = store.tunables();
                let data: Vec<u8> = fs::read(wasm_module_path)?;
                let prefixer: Option<PrefixerFn> = None;
                let (module_info, obj, metadata_length, symbol_registry) =
                    Artifact::generate_object(
                        compiler, &data, prefixer, &target, tunables, features,
                    )?;

                let header_file_src = crate::c_gen::staticlib_header::generate_header_file(
                    &module_info,
                    &*symbol_registry,
                    metadata_length,
                );
                let mut writer = BufWriter::new(File::create(&output_path)?);
                obj.write_stream(&mut writer)
                    .map_err(|err| anyhow::anyhow!(err.to_string()))?;
                writer.flush()?;
                let mut writer = BufWriter::new(File::create(&header_output_path)?);
                writer.write_all(header_file_src.as_bytes())?;
                writer.flush()?;
            }
        }

        eprintln!(
            "✔ Object compiled successfully to `{}` and the header file was generated at `{}`.",
            self.output.display(),
            header_output.display(),
        );

        Ok(())
    }
}
fn link(
    output_path: PathBuf,
    object_path: PathBuf,
    header_code_path: PathBuf,
) -> anyhow::Result<()> {
    let libwasmer_path = get_libwasmer_path()?
        .canonicalize()
        .context("Failed to find libwasmer")?;
    println!(
        "link output {:?}",
        Command::new("cc")
            .arg(&header_code_path)
            .arg(&format!("-L{}", libwasmer_path.display()))
            //.arg(&format!("-I{}", header_code_path.display()))
            .arg("-pie")
            .arg("-o")
            .arg("header_obj.o")
            .output()?
    );
    //ld -relocatable a.o b.o -o c.o

    println!(
        "link output {:?}",
        Command::new("ld")
            .arg("-relocatable")
            .arg(&object_path)
            .arg("header_obj.o")
            .arg("-o")
            .arg(&output_path)
            .output()?
    );

    Ok(())
}

/// path to the static libwasmer
fn get_libwasmer_path() -> anyhow::Result<PathBuf> {
    let mut path = get_wasmer_dir()?;
    path.push("lib");

    // TODO: prefer headless Wasmer if/when it's a separate library.
    #[cfg(not(windows))]
    path.push("libwasmer.a");
    #[cfg(windows)]
    path.push("wasmer.lib");

    Ok(path)
}
fn get_wasmer_dir() -> anyhow::Result<PathBuf> {
    Ok(PathBuf::from(
        env::var("WASMER_DIR")
            .or_else(|e| {
                option_env!("WASMER_INSTALL_PREFIX")
                    .map(str::to_string)
                    .ok_or(e)
            })
            .context("Trying to read env var `WASMER_DIR`")?,
    ))
}
