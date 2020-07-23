use crate::store::StoreOptions;
use anyhow::{Context, Result};
use bytesize::ByteSize;
use std::path::PathBuf;
use structopt::StructOpt;
use wasmer::*;

#[derive(Debug, StructOpt)]
/// The options for the `wasmer validate` subcommand
pub struct Inspect {
    /// File to validate as WebAssembly
    #[structopt(name = "FILE", parse(from_os_str))]
    path: PathBuf,

    #[structopt(flatten)]
    store: StoreOptions,
}

impl Inspect {
    /// Runs logic for the `validate` subcommand
    pub fn execute(&self) -> Result<()> {
        self.inner_execute()
            .context(format!("failed to inspect `{}`", self.path.display()))
    }
    fn inner_execute(&self) -> Result<()> {
        let (store, _engine_type, _compiler_type) = self.store.get_store()?;
        let module_contents = std::fs::read(&self.path)?;
        let module = Module::new(&store, &module_contents)?;
        println!(
            "Type: {}",
            if !is_wasm(&module_contents) {
                "wat"
            } else {
                "wasm"
            }
        );
        println!("Size: {}", ByteSize(module_contents.len() as _));
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
