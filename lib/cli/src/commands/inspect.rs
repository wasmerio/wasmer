use std::path::PathBuf;

use crate::backend::RuntimeOptions;
use anyhow::{Context, Result};
use bytesize::ByteSize;
use clap::Parser;
use wasmer::*;
use wasmer_types::target::Target;

#[derive(Debug, Parser)]
/// The options for the `wasmer validate` subcommand
pub struct Inspect {
    /// File to validate as WebAssembly
    #[clap(name = "FILE")]
    path: PathBuf,

    #[clap(flatten)]
    rt: RuntimeOptions,
}

impl Inspect {
    /// Runs logic for the `validate` subcommand
    pub fn execute(&self) -> Result<()> {
        self.inner_execute()
            .context(format!("failed to inspect `{}`", self.path.display()))
    }

    fn inner_execute(&self) -> Result<()> {
        let mut module_contents = std::fs::read(&self.path)?;
        let iswasm = is_wasm(&module_contents);

        if !iswasm {
            if let Some(extension) = self.path.extension()
                && let Some(extension) = extension.to_str()
                && extension == "wat"
            {
                module_contents = wat2wasm(&module_contents)
                    .map_err(|e| anyhow::anyhow!("Cannot convert WAT to WASM: {e}"))?
                    .to_vec();
            } else {
                anyhow::bail!("The input file is not a WAT file");
            }
        }

        let engine = self
            .rt
            .get_engine_for_module(&module_contents, &Target::default())?;

        let module_len = module_contents.len();
        let module = Module::new(&engine, module_contents)?;
        println!(
            "Backend used to parse the module: {}",
            engine.deterministic_id()
        );

        println!("Type: {}", if !iswasm { "wat" } else { "wasm" });
        println!("Size: {}", ByteSize(module_len as _));
        println!("Imports:");
        println!("  Functions:");
        for f in module.imports().functions() {
            println!("    \"{}\".\"{}\": {}", f.module(), f.name(), f.ty());
        }
        println!("  Memories:");
        for f in module.imports().memories() {
            println!("    \"{}\".\"{}\": {}", f.module(), f.name(), f.ty());
        }
        println!("  Tables:");
        for f in module.imports().tables() {
            println!("    \"{}\".\"{}\": {}", f.module(), f.name(), f.ty());
        }
        println!("  Globals:");
        for f in module.imports().globals() {
            println!("    \"{}\".\"{}\": {}", f.module(), f.name(), f.ty());
        }
        println!("Exports:");
        println!("  Functions:");
        for f in module.exports().functions() {
            println!("    \"{}\": {}", f.name(), f.ty());
        }
        println!("  Memories:");
        for f in module.exports().memories() {
            println!("    \"{}\": {}", f.name(), f.ty());
        }
        println!("  Tables:");
        for f in module.exports().tables() {
            println!("    \"{}\": {}", f.name(), f.ty());
        }
        println!("  Globals:");
        for f in module.exports().globals() {
            println!("    \"{}\": {}", f.name(), f.ty());
        }
        Ok(())
    }
}
