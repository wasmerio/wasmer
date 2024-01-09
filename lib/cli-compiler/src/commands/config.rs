use crate::VERSION;
use anyhow::{Context, Result};
use clap::Parser;
use std::env;
use std::path::PathBuf;

#[derive(Debug, Parser)]
/// The options for the `wasmer config` subcommand
pub struct Config {
    /// Print the installation prefix.
    #[clap(long, conflicts_with = "pkg_config")]
    prefix: bool,

    /// Directory containing Wasmer executables.
    #[clap(long, conflicts_with = "pkg_config")]
    bindir: bool,

    /// Directory containing Wasmer headers.
    #[clap(long, conflicts_with = "pkg_config")]
    includedir: bool,

    /// Directory containing Wasmer libraries.
    #[clap(long, conflicts_with = "pkg_config")]
    libdir: bool,

    /// Libraries needed to link against Wasmer components.
    #[clap(long, conflicts_with = "pkg_config")]
    libs: bool,

    /// C compiler flags for files that include Wasmer headers.
    #[clap(long, conflicts_with = "pkg_config")]
    cflags: bool,

    /// It outputs the necessary details for compiling
    /// and linking a program to Wasmer, using the `pkg-config` format.
    #[clap(long)]
    pkg_config: bool,
}

impl Config {
    /// Runs logic for the `config` subcommand
    pub fn execute(&self) -> Result<()> {
        self.inner_execute()
            .context("failed to retrieve the wasmer config".to_string())
    }
    fn inner_execute(&self) -> Result<()> {
        let key = "WASMER_DIR";
        let wasmer_dir = env::var(key)
            .ok()
            .or_else(|| option_env!("WASMER_INSTALL_PREFIX").map(str::to_string))
            .or_else(|| {
                // Allowing deprecated function home_dir since it works fine,
                // and will never be removed from std.
                #[allow(deprecated)]
                let dir = std::env::home_dir()?.join(".wasmer").to_str()?.to_string();

                Some(dir)
            })
            .context(format!(
                "failed to retrieve the {} environment variables",
                key
            ))?;

        let prefix = PathBuf::from(wasmer_dir);

        let prefixdir = prefix.display().to_string();
        let bindir = prefix.join("bin").display().to_string();
        let includedir = prefix.join("include").display().to_string();
        let libdir = prefix.join("lib").display().to_string();
        let cflags = format!("-I{}", includedir);
        let libs = format!("-L{} -lwasmer", libdir);

        if self.pkg_config {
            println!("prefix={}", prefixdir);
            println!("exec_prefix={}", bindir);
            println!("includedir={}", includedir);
            println!("libdir={}", libdir);
            println!();
            println!("Name: wasmer");
            println!("Description: The Wasmer library for running WebAssembly");
            println!("Version: {}", VERSION);
            println!("Cflags: {}", cflags);
            println!("Libs: {}", libs);
            return Ok(());
        }

        if self.prefix {
            println!("{}", prefixdir);
        }
        if self.bindir {
            println!("{}", bindir);
        }
        if self.includedir {
            println!("{}", includedir);
        }
        if self.libdir {
            println!("{}", libdir);
        }
        if self.libs {
            println!("{}", libs);
        }
        if self.cflags {
            println!("{}", cflags);
        }
        Ok(())
    }
}
