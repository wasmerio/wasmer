use std::{fs, path::Path};

use anyhow::Result;
use clap::Parser;
use wasmer_registry::wasmer_env::WasmerEnv;

#[derive(Debug, Parser)]
/// The options for the `wasmer cache` subcommand
pub struct Cache {
    #[clap(flatten)]
    env: WasmerEnv,
    /// The operation to perform.
    #[clap(subcommand)]
    cmd: Cmd,
}

impl Cache {
    /// Execute the cache command
    pub fn execute(&self) -> Result<()> {
        let cache_dir = self.env.cache_dir();

        match self.cmd {
            Cmd::Clean => {
                clean(&cache_dir)?;
            }
            Cmd::Dir => {
                println!("{}", self.env.cache_dir().display());
            }
        }

        Ok(())
    }
}

#[derive(Debug, Copy, Clone, Parser)]
enum Cmd {
    /// Clear the cache
    Clean,
    /// Display the location of the cache
    Dir,
}

fn clean(cache_dir: &Path) -> Result<()> {
    if cache_dir.exists() {
        fs::remove_dir_all(cache_dir)?;
    }
    fs::create_dir_all(cache_dir)?;
    eprintln!("Wasmer cache cleaned successfully.");

    Ok(())
}
