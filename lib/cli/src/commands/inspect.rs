use std::path::PathBuf;

use anyhow::{Context, Result};
use bytesize::ByteSize;
use clap::Parser;
use wasmer::*;

use crate::store::StoreOptions;

#[derive(Debug, Parser)]
/// The options for the `wasmer validate` subcommand
pub struct Inspect {
    /// File to validate as WebAssembly
    #[clap(name = "FILE")]
    path: PathBuf,

    #[clap(flatten)]
    store: StoreOptions,
}

impl Inspect {
    /// Runs logic for the `validate` subcommand
    pub fn execute(&self) -> Result<()> {
        self.inner_execute()
            .context(format!("failed to inspect `{}`", self.path.display()))
    }

    fn inner_execute(&self) -> Result<()> {
        let (store, _compiler_type) = self.store.get_store()?;
        let module_contents = std::fs::read(&self.path)?;
        let iswasm = is_wasm(&module_contents);
        let module_len = module_contents.len();
        let module = Module::new(&store, module_contents)?;
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
