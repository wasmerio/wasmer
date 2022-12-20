use crate::VERSION;
use anyhow::{Context, Result};
use clap::Parser;
use std::env;
use std::path::PathBuf;
use std::str::ParseBoolError;
use wasmer_registry::WasmerConfig;

#[derive(Debug, Parser)]
/// The options for the `wasmer config` subcommand: `wasmer config get --OPTION` or `wasmer config set [FLAG]`
pub struct Config {
    #[clap(flatten)]
    flags: Flags,
    /// Subcommand for `wasmer config get | set`
    #[clap(subcommand)]
    set: Option<Set>,
}

/// Normal configuration
#[derive(Debug, Parser)]
pub struct Flags {
    /// Print the installation prefix.
    #[clap(long, conflicts_with = "pkg-config")]
    prefix: bool,

    /// Directory containing Wasmer executables.
    #[clap(long, conflicts_with = "pkg-config")]
    bindir: bool,

    /// Directory containing Wasmer headers.
    #[clap(long, conflicts_with = "pkg-config")]
    includedir: bool,

    /// Directory containing Wasmer libraries.
    #[clap(long, conflicts_with = "pkg-config")]
    libdir: bool,

    /// Libraries needed to link against Wasmer components.
    #[clap(long, conflicts_with = "pkg-config")]
    libs: bool,

    /// C compiler flags for files that include Wasmer headers.
    #[clap(long, conflicts_with = "pkg-config")]
    cflags: bool,

    /// Print the path to the wasmer configuration file where all settings are stored
    #[clap(long, conflicts_with = "pkg-config")]
    config_path: bool,

    /// Print the registry URL of the currently active registry
    #[clap(long, conflicts_with = "pkg-config")]
    registry_url: bool,

    /// Print the token for the currently active registry or nothing if not logged in
    #[clap(long, conflicts_with = "pkg-config")]
    registry_token: bool,

    /// Print whether telemetry is currently enabled
    #[clap(long, conflicts_with = "pkg-config")]
    telemetry_enabled: bool,

    /// Print whether update notifications are enabled
    #[clap(long, conflicts_with = "pkg-config")]
    update_notifications_enabled: bool,

    /// Print the proxy URL
    #[clap(long, conflicts_with = "pkg-config")]
    proxy_url: bool,

    /// Outputs the necessary details for compiling
    /// and linking a program to Wasmer, using the `pkg-config` format.
    #[clap(long)]
    pkg_config: bool,
}

/// Subcommand for `wasmer config set`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Parser)]
pub enum Set {
    /// `wasmer config set $KEY $VALUE`
    #[clap(subcommand)]
    Set(StorableConfigField),
}

/// Setting that can be stored in the wasmer config
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Parser)]
pub enum StorableConfigField {
    /// Print the registry URL of the currently active registry
    #[clap(name = "registry.url")]
    RegistryUrl(SetRegistryUrl),
    /// Print the token for the currently active registry or nothing if not logged in
    #[clap(name = "registry.token")]
    RegistryToken(SetRegistryToken),
    /// Print whether telemetry is currently enabled
    #[clap(name = "telemetry.enabled")]
    TelemetryEnabled(SetTelemetryEnabled),
    /// Print whether update notifications are enabled
    #[clap(name = "update-notifications.enabled")]
    UpdateNotificationsEnabled(SetUpdateNotificationsEnabled),
    /// Print the proxy URL
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
    #[clap(name = "ENABLED", possible_values = ["true", "false"])]
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
    #[clap(name = "ENABLED", possible_values = ["true", "false"])]
    pub enabled: BoolString,
}

/// Set if a proxy URL should be used
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Parser)]
pub struct SetProxyUrl {
    /// Set if a proxy URL should be used (empty = unset proxy)
    #[clap(name = "URL")]
    pub url: Option<String>,
}

impl Config {
    /// Runs logic for the `config` subcommand
    pub fn execute(&self) -> Result<()> {
        self.inner_execute()
            .context("failed to retrieve the wasmer config".to_string())
    }
    fn inner_execute(&self) -> Result<()> {
        if let Some(Set::Set(s)) = self.set.as_ref() {
            return s.execute();
        }

        let flags = &self.flags;

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
            let path = WasmerConfig::get_file_location()
                .map_err(|e| anyhow::anyhow!("could not find config file: {e}"))?;
            println!("{}", path.display());
        }

        let config = WasmerConfig::from_file()
            .map_err(|e| anyhow::anyhow!("could not find config file: {e}"))?;

        if flags.registry_url {
            println!("{}", config.registry.get_current_registry());
        }

        if flags.registry_token {
            if let Some(s) = config
                .registry
                .get_login_token_for_registry(&config.registry.get_current_registry())
            {
                println!("{s}");
            }
        }

        if flags.telemetry_enabled {
            println!("{:?}", config.telemetry_enabled);
        }

        if flags.update_notifications_enabled {
            println!("{:?}", config.update_notifications_enabled);
        }

        if flags.proxy_url {
            if let Some(s) = config.proxy.url.as_ref() {
                println!("{s}");
            }
        }

        Ok(())
    }
}

impl StorableConfigField {
    fn execute(&self) -> Result<()> {
        let config_file = WasmerConfig::get_file_location()
            .map_err(|e| anyhow::anyhow!("could not find config file {e}"))?;
        let mut config = WasmerConfig::from_file().map_err(|e| {
            anyhow::anyhow!(
                "could not find config file {e} at {}",
                config_file.display()
            )
        })?;
        match self {
            StorableConfigField::RegistryUrl(s) => {
                config.registry.set_current_registry(&s.url);
                let current_registry = config.registry.get_current_registry();
                if let Some(u) = wasmer_registry::utils::get_username().ok().and_then(|o| o) {
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
                config.proxy.url = p.url.clone();
            }
            StorableConfigField::UpdateNotificationsEnabled(u) => {
                config.update_notifications_enabled = u.enabled.0;
            }
        }
        config
            .save(config_file)
            .with_context(|| anyhow::anyhow!("could not save config file"))?;

        Ok(())
    }
}
