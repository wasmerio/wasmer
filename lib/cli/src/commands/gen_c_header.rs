use std::path::PathBuf;

use anyhow::{Context, Error};
use bytes::Bytes;
use clap::Parser;
use wasmer_compiler::Artifact;
use wasmer_types::{
    compilation::symbols::ModuleMetadataSymbolRegistry, CpuFeature, MetadataHeader, Triple,
};
use webc::{compat::SharedBytes, Container, DetectError};

use crate::store::CompilerOptions;

#[derive(Debug, Parser)]
/// The options for the `wasmer gen-c-header` subcommand
pub struct GenCHeader {
    /// Input file
    #[clap(name = "FILE")]
    path: PathBuf,

    /// Prefix hash (default: SHA256 of input .wasm file)
    #[clap(long)]
    prefix: Option<String>,

    /// For pirita files: optional atom name to compile
    #[clap(long)]
    atom: Option<String>,

    /// Output file
    #[clap(name = "OUTPUT PATH", short = 'o')]
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

    #[clap(long, short = 'm', number_of_values = 1)]
    cpu_features: Vec<CpuFeature>,
}

impl GenCHeader {
    /// Runs logic for the `gen-c-header` subcommand
    pub fn execute(&self) -> Result<(), Error> {
        let file: Bytes = std::fs::read(&self.path)
            .with_context(|| format!("Unable to read \"{}\"", self.path.display()))?
            .into();
        let prefix = match self.prefix.as_deref() {
            Some(s) => s.to_string(),
            None => crate::commands::PrefixMapCompilation::hash_for_bytes(&file),
        };

        let atom = match Container::from_bytes(file.clone()) {
            Ok(webc) => self.get_atom(&webc)?,
            Err(webc::compat::ContainerError::Detect(DetectError::InvalidMagic { .. })) => {
                // we've probably got a WebAssembly file
                file.into()
            }
            Err(other) => {
                return Err(Error::new(other).context("Unable to parse the webc file"));
            }
        };

        let target_triple = self.target_triple.clone().unwrap_or_else(Triple::host);
        let target = crate::commands::create_exe::utils::target_triple_to_target(
            &target_triple,
            &self.cpu_features,
        );
        let (engine, _) = CompilerOptions::default().get_engine_for_target(target.clone())?;
        let engine_inner = engine.inner();
        let compiler = engine_inner.compiler()?;
        let features = engine_inner.features();
        let tunables = engine.tunables();
        let (metadata, _, _) = Artifact::metadata(
            compiler,
            &atom,
            Some(prefix.as_str()),
            &target,
            tunables,
            features,
        )
        .map_err(|e| anyhow::anyhow!("could not generate metadata: {e}"))?;

        let serialized_data = metadata
            .serialize()
            .map_err(|e| anyhow::anyhow!("failed to serialize: {e}"))?;
        let mut metadata_binary = vec![];
        metadata_binary.extend(MetadataHeader::new(serialized_data.len()).into_bytes());
        metadata_binary.extend(serialized_data);
        let metadata_length = metadata_binary.len();

        let header_file_src = crate::c_gen::staticlib_header::generate_header_file(
            &prefix,
            &metadata.compile_info.module,
            &ModuleMetadataSymbolRegistry {
                prefix: prefix.clone(),
            },
            metadata_length,
        );

        let output = crate::common::normalize_path(&self.output.display().to_string());

        std::fs::write(&output, header_file_src)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .with_context(|| anyhow::anyhow!("{output}"))?;

        Ok(())
    }

    fn get_atom(&self, pirita: &Container) -> Result<SharedBytes, Error> {
        let atoms = pirita.atoms();
        let atom_names: Vec<_> = atoms.keys().map(|s| s.as_str()).collect();

        match *atom_names.as_slice() {
            [] => Err(Error::msg("The file doesn't contain any atoms")),
            [name] => Ok(atoms[name].clone()),
            [..] => match &self.atom {
                Some(name) => atoms
                    .get(name)
                    .cloned()
                    .with_context(|| format!("The file doesn't contain a \"{name}\" atom"))
                    .with_context(|| {
                        format!("-> note: available atoms are: {}", atom_names.join(", "))
                    }),
                None => {
                    let err = Error::msg("file has multiple atoms, please specify which atom to generate the header file for")
                            .context(format!("-> note: available atoms are: {}", atom_names.join(", ")));
                    Err(err)
                }
            },
        }
    }
}
