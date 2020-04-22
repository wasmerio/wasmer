use crate::common::get_cache_dir;
use anyhow::Result;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
/// The options for the `wasmer cache` subcommand
pub enum Cache {
    /// Clear the cache
    #[structopt(name = "clean")]
    Clean,

    /// Display the location of the cache
    #[structopt(name = "dir")]
    Dir,
}

impl Cache {
    /// Execute the cache command
    pub fn execute(&self) -> Result<()> {
        match &self {
            Cache::Clean => {
                use std::fs;
                let cache_dir = get_cache_dir();
                if cache_dir.exists() {
                    fs::remove_dir_all(cache_dir.clone())?;
                }
                fs::create_dir_all(cache_dir.clone())?;
            }
            Cache::Dir => {
                println!("{}", get_cache_dir().to_string_lossy());
            }
        }
        Ok(())
    }
}
