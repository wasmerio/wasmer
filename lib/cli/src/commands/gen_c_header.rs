use crate::store::CompilerOptions;
use anyhow::Context;
use clap::Parser;
use std::path::PathBuf;
use wasmer_compiler::Artifact;
use wasmer_types::compilation::symbols::ModuleMetadataSymbolRegistry;
use wasmer_types::{CpuFeature, MetadataHeader, Triple};
use webc::v1::WebC;

#[derive(Debug, Parser)]
/// The options for the `wasmer gen-c-header` subcommand
pub struct GenCHeader {
    /// Input file
    #[clap(name = "FILE", parse(from_os_str))]
    path: PathBuf,

    /// Prefix hash (default: SHA256 of input .wasm file)
    #[clap(long)]
    prefix: Option<String>,

    /// For pirita files: optional atom name to compile
    #[clap(long)]
    atom: Option<String>,

    /// Output file
    #[clap(name = "OUTPUT PATH", short = 'o', parse(from_os_str))]
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

    #[clap(long, short = 'm', multiple = true, number_of_values = 1)]
    cpu_features: Vec<CpuFeature>,
}

impl GenCHeader {
    /// Runs logic for the `gen-c-header` subcommand
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        let path = crate::common::normalize_path(&format!("{}", self.path.display()));
        let mut file = std::fs::read(&path)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .with_context(|| anyhow::anyhow!("{path}"))?;
        let prefix = match self.prefix.as_deref() {
            Some(s) => s.to_string(),
            None => crate::commands::PrefixMapCompilation::hash_for_bytes(&file),
        };

        if let Ok(pirita) = WebC::parse(&file, &webc::v1::ParseOptions::default()) {
            let atoms = pirita
                .manifest
                .atoms
                .iter()
                .map(|a| a.0.clone())
                .collect::<Vec<_>>();
            if atoms.len() == 1 {
                file = pirita
                    .get_atom(&pirita.get_package_name(), &atoms[0])
                    .unwrap()
                    .to_vec();
            } else if self.atom.is_none() {
                return Err(anyhow::anyhow!("-> note: available atoms are: {}", atoms.join(", ")))
                .context(anyhow::anyhow!("file has multiple atoms, please specify which atom to generate the header file for"))?;
            } else {
                file = pirita
                    .get_atom(&pirita.get_package_name(), &atoms[0])
                    .map_err(|_| {
                        anyhow::anyhow!("-> note: available atoms are: {}", atoms.join(", "))
                    })
                    .context(anyhow::anyhow!(
                        "could not get atom {} from file (invalid atom name)",
                        &atoms[0]
                    ))?
                    .to_vec();
            }
        }

        let target_triple = self.target_triple.clone().unwrap_or_else(Triple::host);
        let target = crate::commands::create_exe::utils::target_triple_to_target(
            &target_triple,
            &self.cpu_features,
        );
        let (store, _) = CompilerOptions::default().get_store_for_target(target.clone())?;
        let engine = store.engine();
        let engine_inner = engine.inner();
        let compiler = engine_inner.compiler()?;
        let features = engine_inner.features();
        let tunables = store.tunables();
        let (metadata, _, _) = Artifact::metadata(
            compiler,
            &file,
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

        std::fs::write(&output, &header_file_src)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .with_context(|| anyhow::anyhow!("{output}"))?;

        Ok(())
    }
}
