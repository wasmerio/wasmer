#![allow(dead_code)]
//! Create a standalone native executable for a given Wasm file.

use super::ObjectFormat;
use crate::store::CompilerOptions;
use anyhow::Result;
use clap::Parser;
use std::env;

use std::path::PathBuf;

use wasmer::*;

#[cfg(feature = "webc_runner")]
use webc::{ParseOptions, WebCMmap};

#[derive(Debug, Parser)]
/// The options for the `wasmer create-exe` subcommand
pub struct CreateObj {
    /// Input file
    #[clap(name = "FILE", parse(from_os_str))]
    path: PathBuf,

    /// Output file or directory if the input is a pirita file
    #[clap(name = "OUTPUT_PATH", short = 'o', parse(from_os_str))]
    output: PathBuf,

    /// Optional directorey used for debugging: if present, will
    /// output the files to a debug instead of a temp directory
    #[clap(long, name = "DEBUG PATH", parse(from_os_str))]
    debug_dir: Option<PathBuf>,

    /// Prefix for every input file, e.g. "wat2wasm:sha256abc123" would
    /// prefix every function in the wat2wasm input object with the "sha256abc123" hash
    ///
    /// If only a single value is given without containing a ":", this value is used for
    /// all input files. If no value is given, the prefix is always equal to
    /// the sha256 of the input .wasm file
    #[clap(
        use_value_delimiter = true,
        value_delimiter = ',',
        name = "FILE:PREFIX"
    )]
    prefix: Vec<String>,

    /// Compilation Target triple
    ///
    /// Accepted target triple values must follow the
    /// ['target_lexicon'](https://crates.io/crates/target-lexicon) crate format.
    ///
    /// The recommended targets we try to support are:
    ///
    /// - "x86_64-linux-gnu"
    /// - "aarch64-linux-gnu"
    /// - "x86_64-apple-darwin"
    /// - "arm64-apple-darwin"
    /// - "x86_64-windows-gnu"
    #[clap(long = "target")]
    target_triple: Option<Triple>,

    /// Object format options
    ///
    /// This flag accepts two options: `symbols` or `serialized`.
    /// - (default) `symbols` creates an object where all functions and metadata of the module are regular object symbols
    /// - `serialized` creates an object where the module is zero-copy serialized as raw data
    #[clap(long = "object-format", name = "OBJECT_FORMAT", verbatim_doc_comment)]
    object_format: Option<ObjectFormat>,

    #[clap(long, short = 'm', multiple = true, number_of_values = 1)]
    cpu_features: Vec<CpuFeature>,

    #[clap(flatten)]
    compiler: CompilerOptions,
}

impl CreateObj {
    /// Runs logic for the `create-obj` subcommand
    pub fn execute(&self) -> Result<()> {
        let target_triple = self.target_triple.clone().unwrap_or_else(Triple::host);
        let starting_cd = env::current_dir()?;
        let input_path = starting_cd.join(&self.path);
        let temp_dir = tempdir::TempDir::new("create-obj-intermediate")?;
        let output_directory_path = match self.debug_dir.as_ref() {
            Some(s) => s.as_path(),
            None => temp_dir.path(),
        };
        let object_format = self.object_format.unwrap_or_default();
        if let Ok(pirita) = WebCMmap::parse(input_path.clone(), &ParseOptions::default()) {
            crate::commands::create_exe::compile_pirita_into_directory(
                &pirita,
                output_directory_path,
                &self.compiler,
                &self.cpu_features,
                &target_triple,
                object_format,
                &self.prefix,
            )?;
        } else {
            crate::commands::create_exe::prepare_directory_from_single_wasm_file(
                &input_path,
                output_directory_path,
                &self.compiler,
                &target_triple,
                &self.cpu_features,
                object_format,
                &self.prefix,
            )?;
        }

        // Copy output files into target path, depending on whether
        // there are one or many files being compiled
        let file_paths = std::fs::read_dir(output_directory_path.join("atoms"))
            .map_err(|e| {
                anyhow::anyhow!(
                    "could not read {}: {e}",
                    output_directory_path.join("atoms").display()
                )
            })?
            .filter_map(|path| Some(path.ok()?.path()))
            .collect::<Vec<_>>();

        if file_paths.is_empty() {
            return Err(anyhow::anyhow!(
                "could not compile object file: no output objects in {}",
                output_directory_path.join("atoms").display()
            ));
        }

        if file_paths.len() == 1 {
            std::fs::copy(&file_paths[0], &self.output)?;
        }

        eprintln!(
            "✔ Object compiled successfully to directory `{}`",
            self.output.display()
        );

        Ok(())
    }
}
