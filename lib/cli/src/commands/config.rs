use crate::VERSION;
use anyhow::{Context, Result};
use clap::Parser;
use std::env;
use std::path::PathBuf;
use wasmer_registry::PartialWapmConfig;

#[derive(Debug, Parser)]
/// The options for the `wasmer config` subcommand: `wasmer config get prefix`
pub enum Config {
    /// Get a value from the current wasmer config
    #[clap(subcommand)]
    Get(RetrievableConfigField),
    /// Set a value in the current wasmer config
    #[clap(subcommand)]
    Set(StorableConfigField),
}

/// Value that can be queried from the wasmer config
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, clap::Subcommand)]
pub enum RetrievableConfigField {
    /// `prefix`
    Prefix,
    /// `bin-dir`
    Bindir,
    /// `includedir`
    Includedir,
    /// `libdir`
    Libdir,
    /// `libs`
    Libs,
    /// `cflags`
    Cflags,
    /// `pkg-config`
    PkgConfig,
}

/// Setting that can be stored in the wasmer config
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, clap::Subcommand)]
pub enum StorableConfigField {
    /// `registry.url`
    #[clap(name = "registry.url")]
    RegistryUrl(SetRegistryUrl),
}

/// Set a new registry URL
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Parser)]
pub struct SetRegistryUrl {
    /// Url of the registry
    #[clap(name = "URL")]
    pub url: String,
}

impl Config {
    /// Runs logic for the `config` subcommand
    pub fn execute(&self) -> Result<()> {
        self.inner_execute()
            .context("failed to retrieve the wasmer config".to_string())
    }
    fn inner_execute(&self) -> Result<()> {
        use self::Config::{Get, Set};

        let key = "WASMER_DIR";
        let wasmer_dir = env::var(key)
            .or_else(|e| {
                option_env!("WASMER_INSTALL_PREFIX")
                    .map(str::to_string)
                    .ok_or(e)
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

        match self {
            Get(g) => match g {
                RetrievableConfigField::PkgConfig => {
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
                }
                RetrievableConfigField::Prefix => {
                    println!("{}", prefixdir);
                }
                RetrievableConfigField::Bindir => {
                    println!("{}", bindir);
                }
                RetrievableConfigField::Includedir => {
                    println!("{}", includedir);
                }
                RetrievableConfigField::Libdir => {
                    println!("{}", libdir);
                }
                RetrievableConfigField::Libs => {
                    println!("{}", libs);
                }
                RetrievableConfigField::Cflags => {
                    println!("{}", cflags);
                }
            },
            Set(s) => match s {
                StorableConfigField::RegistryUrl(s) => {
                    let config_file = PartialWapmConfig::get_file_location()
                        .map_err(|e| anyhow::anyhow!("could not find config file {e}"))?;
                    let mut config = PartialWapmConfig::from_file()
                        .map_err(|e| anyhow::anyhow!("could not find config file {e}"))?;
                    config.registry.set_current_registry(&s.url);
                    config
                        .save(config_file)
                        .with_context(|| anyhow::anyhow!("could not save config file"))?;
                    println!(
                        "set current registry to {}",
                        config.registry.get_current_registry()
                    );
                }
            },
        }
        Ok(())
    }
}
