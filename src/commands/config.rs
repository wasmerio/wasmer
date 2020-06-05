use anyhow::{Context, Result};
use std::env;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
/// The options for the `wasmer config` subcommand
pub struct Config {
    /// Print the installation prefix.
    #[structopt(long)]
    prefix: bool,

    /// Directory containing Wasmer executables.
    #[structopt(long)]
    bindir: bool,

    /// Directory containing Wasmer headers.
    #[structopt(long)]
    includedir: bool,

    /// Directory containing Wasmer libraries.
    #[structopt(long)]
    libdir: bool,
}

impl Config {
    /// Runs logic for the `config` subcommand
    pub fn execute(&self) -> Result<()> {
        self.inner_execute()
            .context(format!("failed to retrieve the wasmer config"))
    }
    fn inner_execute(&self) -> Result<()> {
        let key = "WASMER_DIR";
        let wasmer_dir = env::var(key).context(format!(
            "failed to retrieve the {} environment variable",
            key
        ))?;
        let mut prefix = PathBuf::new();
        prefix.push(wasmer_dir);

        if self.prefix {
            println!("{}", prefix.display());
        }
        if self.bindir {
            let mut bindir = prefix.clone();
            bindir.push("bin");
            println!("{}", bindir.display());
        }
        if self.includedir {
            let mut includedir = prefix.clone();
            includedir.push("include");
            println!("{}", includedir.display());
        }
        if self.libdir {
            let mut libdir = prefix.clone();
            libdir.push("lib");
            println!("{}", libdir.display());
        }
        Ok(())
    }
}
