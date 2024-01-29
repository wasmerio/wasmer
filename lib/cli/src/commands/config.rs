use std::str::ParseBoolError;

use anyhow::{Context, Result};
use clap::Parser;
use wasmer_registry::{wasmer_env::WasmerEnv, WasmerConfig};

use crate::VERSION;

#[derive(Debug, Parser)]
/// The options for the `wasmer config` subcommand: `wasmer config get --OPTION` or `wasmer config set [FLAG]`
pub struct Config {
    #[clap(flatten)]
    env: WasmerEnv,

    #[clap(flatten)]
    flags: Flags,
    /// Subcommand for `wasmer config get | set`
    #[clap(subcommand)]
    set: Option<GetOrSet>,
}

/// Normal configuration
#[derive(Debug, Parser)]
pub struct Flags {
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

    /// Print the path to the wasmer configuration file where all settings are stored
    #[clap(long, conflicts_with = "pkg_config")]
    config_path: bool,

    /// Outputs the necessary details for compiling
    /// and linking a program to Wasmer, using the `pkg-config` format.
    #[clap(long)]
    pkg_config: bool,
}

/// Subcommand for `wasmer config set`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Parser)]
pub enum GetOrSet {
    /// `wasmer config get $KEY`
    #[clap(subcommand)]
    Get(RetrievableConfigField),
    /// `wasmer config set $KEY $VALUE`
    #[clap(subcommand)]
    Set(StorableConfigField),
}

/// Subcommand for `wasmer config get`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Parser)]
pub enum RetrievableConfigField {
    /// Print the registry URL of the currently active registry
    #[clap(name = "registry.url")]
    RegistryUrl,
    /// Print the token for the currently active registry or nothing if not logged in
    #[clap(name = "registry.token")]
    RegistryToken,
    /// Print whether telemetry is currently enabled
    #[clap(name = "telemetry.enabled")]
    TelemetryEnabled,
    /// Print whether update notifications are enabled
    #[clap(name = "update-notifications.enabled")]
    UpdateNotificationsEnabled,
    /// Print the proxy URL
    #[clap(name = "proxy.url")]
    ProxyUrl,
}

/// Setting that can be stored in the wasmer config
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Parser)]
pub enum StorableConfigField {
    /// Set the registry URL of the currently active registry
    #[clap(name = "registry.url")]
    RegistryUrl(SetRegistryUrl),
    /// Set the token for the currently active registry or nothing if not logged in
    #[clap(name = "registry.token")]
    RegistryToken(SetRegistryToken),
    /// Set whether telemetry is currently enabled
    #[clap(name = "telemetry.enabled")]
    TelemetryEnabled(SetTelemetryEnabled),
    /// Set whether update notifications are enabled
    #[clap(name = "update-notifications.enabled")]
    UpdateNotificationsEnabled(SetUpdateNotificationsEnabled),
    /// Set the active proxy URL
    #[clap(name = "proxy.url")]
    ProxyUrl(SetProxyUrl),
}

/// Set the current active registry URL
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Parser)]
pub struct SetRegistryUrl {
    /// Url of the registry
    #[clap(name = "URL")]
    pub url: String,
}

/// Set or change the token for the current active registry
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Parser)]
pub struct SetRegistryToken {
    /// Token to set
    #[clap(name = "TOKEN")]
    pub token: String,
}

/// Set if update notifications are enabled
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Parser)]
pub struct SetUpdateNotificationsEnabled {
    /// Whether to enable update notifications
    ///
    /// ("true" | "false")
    #[clap(name = "ENABLED")]
    pub enabled: BoolString,
}

/// "true" or "false" for handling input in the CLI
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BoolString(pub bool);

impl std::str::FromStr for BoolString {
    type Err = ParseBoolError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(bool::from_str(s)?))
    }
}

/// Set if telemetry is enabled
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Parser)]
pub struct SetTelemetryEnabled {
    /// Whether to enable telemetry
    ///
    /// ("true" | "false")
    #[clap(name = "ENABLED")]
    pub enabled: BoolString,
}

/// Set if a proxy URL should be used
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Parser)]
pub struct SetProxyUrl {
    /// Set if a proxy URL should be used (empty = unset proxy)
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
        if let Some(s) = self.set.as_ref() {
            return s.execute(&self.env);
        }

        let flags = &self.flags;

        let prefix = self.env.dir();

        let prefixdir = prefix.display().to_string();
        let bindir = prefix.join("bin").display().to_string();
        let includedir = prefix.join("include").display().to_string();
        let libdir = prefix.join("lib").display().to_string();
        let cflags = format!("-I{}", includedir);
        let libs = format!("-L{} -lwasmer", libdir);

        if flags.pkg_config {
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

        if flags.prefix {
            println!("{}", prefixdir);
        }
        if flags.bindir {
            println!("{}", bindir);
        }
        if flags.includedir {
            println!("{}", includedir);
        }
        if flags.libdir {
            println!("{}", libdir);
        }
        if flags.libs {
            println!("{}", libs);
        }
        if flags.cflags {
            println!("{}", cflags);
        }

        if flags.config_path {
            let path = WasmerConfig::get_file_location(self.env.dir());
            println!("{}", path.display());
        }

        Ok(())
    }
}

impl GetOrSet {
    fn execute(&self, env: &WasmerEnv) -> Result<()> {
        let config_file = WasmerConfig::get_file_location(env.dir());
        let mut config = env.config()?;

        match self {
            GetOrSet::Get(g) => match g {
                RetrievableConfigField::RegistryUrl => {
                    println!("{}", config.registry.get_current_registry());
                }
                RetrievableConfigField::RegistryToken => {
                    if let Some(s) = config
                        .registry
                        .get_login_token_for_registry(&config.registry.get_current_registry())
                    {
                        println!("{s}");
                    }
                }
                RetrievableConfigField::TelemetryEnabled => {
                    println!("{:?}", config.telemetry_enabled);
                }
                RetrievableConfigField::UpdateNotificationsEnabled => {
                    println!("{:?}", config.update_notifications_enabled);
                }
                RetrievableConfigField::ProxyUrl => {
                    if let Some(s) = config.proxy.url.as_ref() {
                        println!("{s}");
                    } else {
                        println!("none");
                    }
                }
            },
            GetOrSet::Set(s) => {
                match s {
                    StorableConfigField::RegistryUrl(s) => {
                        config.registry.set_current_registry(&s.url);
                        let current_registry = config.registry.get_current_registry();
                        if let Some(u) = wasmer_registry::utils::get_username(&current_registry)
                            .ok()
                            .and_then(|o| o)
                        {
                            println!(
                                "Successfully logged into registry {current_registry:?} as user {u:?}"
                            );
                        }
                    }
                    StorableConfigField::RegistryToken(t) => {
                        config.registry.set_login_token_for_registry(
                            &config.registry.get_current_registry(),
                            &t.token,
                            wasmer_registry::config::UpdateRegistry::LeaveAsIs,
                        );
                    }
                    StorableConfigField::TelemetryEnabled(t) => {
                        config.telemetry_enabled = t.enabled.0;
                    }
                    StorableConfigField::ProxyUrl(p) => {
                        if p.url == "none" || p.url.is_empty() {
                            config.proxy.url = None;
                        } else {
                            config.proxy.url = Some(p.url.clone());
                        }
                    }
                    StorableConfigField::UpdateNotificationsEnabled(u) => {
                        config.update_notifications_enabled = u.enabled.0;
                    }
                }
                config
                    .save(config_file)
                    .with_context(|| anyhow::anyhow!("could not save config file"))?;
            }
        }
        Ok(())
    }
}
