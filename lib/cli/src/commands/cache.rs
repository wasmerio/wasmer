use std::{fs, path::Path};

use anyhow::Result;
use clap::Parser;

use crate::opts::DirOpts;

#[derive(Debug, Parser)]
/// The options for the `wasmer cache` subcommand
pub struct Cache {
    #[clap(flatten)]
    cache: DirOpts,

    /// The operation to perform.
    #[clap(subcommand)]
    cmd: Cmd,
}

impl Cache {
    /// Execute the cache command
    pub fn execute(&self) -> Result<()> {
        let cache_dir = self.cache.cache_dir()?;

        match self.cmd {
            Cmd::Clean => {
                clean(&cache_dir)?;
            }
            Cmd::Dir => {
                println!("{}", cache_dir.display());
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
