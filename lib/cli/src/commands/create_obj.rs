#![allow(dead_code)]
//! Create a standalone native executable for a given Wasm file.

use super::ObjectFormat;
use crate::{commands::PrefixerFn, store::CompilerOptions};
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
#[cfg(feature = "webc_runner")]
use webc::{ParseOptions, WebCMmap};

const WASMER_SERIALIZED_HEADER: &[u8] = include_bytes!("wasmer_create_exe.h");

#[derive(Debug, Parser)]
/// The options for the `wasmer create-exe` subcommand
pub struct CreateObj {
    /// Input file
    #[clap(name = "FILE", parse(from_os_str))]
    path: PathBuf,

    /// Output file
    #[clap(name = "OUTPUT_PATH", short = 'o', parse(from_os_str))]
    output: PathBuf,

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
        let target_triple = self.target_triple.clone().unwrap_or_else(|| Triple::host());
        let starting_cd = env::current_dir()?;
        let input_path = starting_cd.join(&self.path);
        let output_path = starting_cd.join(&self.output);
        let object_format = self.object_format.unwrap_or_default();
        if let Ok(pirita) = WebCMmap::parse(input_path.clone(), &ParseOptions::default()) {
            crate::commands::create_exe::compile_pirita_into_directory(
                &pirita,
                &output_path,
                &self.compiler,
                &self.cpu_features,
                &target_triple,
                object_format,
            )?;
        } else {
            crate::commands::create_exe::prepare_directory_from_single_wasm_file(
                &input_path,
                &output_path,
                &self.compiler,
                &target_triple,
                &self.cpu_features,
                object_format,
            )?;
        }

        eprintln!(
            "✔ Object compiled successfully to directory `{}`",
            self.output.display()
        );

        Ok(())
    }
}
