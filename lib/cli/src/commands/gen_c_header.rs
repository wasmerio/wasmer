use crate::store::CompilerOptions;
use anyhow::Context;
use clap::Parser;
use std::path::PathBuf;
use wasmer::Target;
use wasmer_compiler::Artifact;
use wasmer_types::compilation::symbols::ModuleMetadataSymbolRegistry;
use webc::WebC;

use super::normalize_path;

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
}

impl GenCHeader {
    /// Runs logic for the `gen-c-header` subcommand
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        let path = crate::commands::normalize_path(&format!("{}", self.path.display()));
        let mut file = std::fs::read(&path)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .with_context(|| anyhow::anyhow!("{path}"))?;
        let prefix = match self.prefix.as_deref() {
            Some(s) => s.to_string(),
            None => crate::commands::PrefixMapCompilation::hash_for_bytes(&file),
        };

        if let Ok(pirita) = WebC::parse(&file, &webc::ParseOptions::default()) {
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

        let (store, _) = CompilerOptions::default().get_store_for_target(Target::default())?;
        let module_name = format!("WASMER_{}_METADATA", prefix.to_uppercase());
        let engine = store.engine();
        let engine_inner = engine.inner();
        let compiler = engine_inner.compiler()?;
        let features = engine_inner.features();
        let tunables = store.tunables();
        let (compile_info, _, _, _) =
            Artifact::generate_metadata(&file, compiler, tunables, features)?;
        let module_info = compile_info.module;

        let metadata_length = 0;

        let header_file_src = crate::c_gen::staticlib_header::generate_header_file(
            &prefix,
            &module_name,
            &module_info,
            &ModuleMetadataSymbolRegistry {
                prefix: prefix.clone(),
            },
            metadata_length as usize,
        );

        let output = normalize_path(&self.output.display().to_string());

        std::fs::write(&output, &header_file_src)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .with_context(|| anyhow::anyhow!("{output}"))?;

        Ok(())
    }
}
