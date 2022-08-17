use crate::common::get_cache_dir;
use anyhow::{Context, Result};
use clap::Parser;
use std::fs;

#[derive(Debug, Parser)]
/// The options for the `wasmer cache` subcommand
pub enum Cache {
    /// Clear the cache
    #[clap(name = "clean")]
    Clean,

    /// Display the location of the cache
    #[clap(name = "dir")]
    Dir,
}

impl Cache {
    /// Execute the cache command
    pub fn execute(&self) -> Result<()> {
        match &self {
            Cache::Clean => {
                self.clean().context("failed to clean wasmer cache.")?;
            }
            Cache::Dir => {
                self.dir()?;
            }
        }
        Ok(())
    }
    fn clean(&self) -> Result<()> {
        let cache_dir = get_cache_dir();
        if cache_dir.exists() {
            fs::remove_dir_all(cache_dir.clone())?;
        }
        fs::create_dir_all(cache_dir)?;
        eprintln!("Wasmer cache cleaned successfully.");
        Ok(())
    }
    fn dir(&self) -> Result<()> {
        println!("{}", get_cache_dir().to_string_lossy());
        Ok(())
    }
}
