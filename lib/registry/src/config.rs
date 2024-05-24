use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use url::Url;

pub static GLOBAL_CONFIG_DATABASE_FILE_NAME: &str = "wasmer.sqlite";

#[derive(Deserialize, Default, Serialize, Debug, PartialEq, Eq)]
pub struct WasmerConfig {
    /// Whether or not telemetry is enabled.
    #[serde(default)]
    pub telemetry_enabled: bool,

    /// Whether or not updated notifications are enabled.
    #[serde(default)]
    pub update_notifications_enabled: bool,

    /// The registry that wasmer will connect to.
    pub registry: MultiRegistry,

    /// The proxy to use when connecting to the Internet.
    #[serde(default)]
    pub proxy: Proxy,
}

pub const fn wax_default_cooldown() -> i32 {
    5 * 60
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq, Default)]
pub struct Proxy {
    pub url: Option<String>,
}

/// Struct to store login tokens for multiple registry URLs
/// inside of the wasmer.toml configuration file
#[derive(Deserialize, Serialize, Debug, PartialEq, Eq, Clone)]
pub struct MultiRegistry {
    /// Currently active registry
    pub active_registry: String,
    /// Map from "RegistryUrl" to "LoginToken", in order to
    /// be able to be able to easily switch between registries
    pub tokens: Vec<RegistryLogin>,
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq, Clone)]
pub struct RegistryLogin {
    /// Registry URL to login to
    pub registry: String,
    /// Login token for the registry
    pub token: String,
}

impl Default for MultiRegistry {
    fn default() -> Self {
        MultiRegistry {
            active_registry: format_graphql("wasmer.io"),
            tokens: Vec::new(),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq, Clone)]
pub struct Registry {
    pub url: String,
    pub token: Option<String>,
}

pub fn format_graphql(registry: &str) -> String {
    if let Ok(mut url) = Url::parse(registry) {
        // Looks like we've got a valid URL. Let's try to use it as-is.
        if url.has_host() {
            if url.path() == "/" {
                // make sure we convert http://registry.wasmer.io/ to
                // http://registry.wasmer.io/graphql
                url.set_path("/graphql");
            }

            return url.to_string();
        }
    }

    if !registry.contains("://") && !registry.contains('/') {
        return endpoint_from_domain_name(registry);
    }

    // looks like we've received something we can't deal with. Just pass it
    // through as-is and hopefully it'll either work or the end user can figure
    // it out
    registry.to_string()
}

/// By convention, something like `"wasmer.io"` should be converted to
/// `"https://registry.wasmer.io/graphql"`.
fn endpoint_from_domain_name(domain_name: &str) -> String {
    if domain_name.contains("localhost") {
        return format!("http://{domain_name}/graphql");
    }

    format!("https://registry.{domain_name}/graphql")
}

fn test_if_registry_present(registry: &str) -> Result<(), String> {
    crate::utils::get_username_registry_token(registry, "")
        .map(|_| ())
        .map_err(|e| format!("{e}"))
}

#[derive(PartialEq, Eq, Copy, Clone)]
pub enum UpdateRegistry {
    Update,
    LeaveAsIs,
}

impl MultiRegistry {
    /// Gets the current (active) registry URL
    pub fn clear_current_registry_token(&mut self) {
        let MultiRegistry {
            active_registry,
            tokens,
        } = self;
        tokens.retain(|i| i.registry != *active_registry);
        tokens.retain(|i| i.registry != format_graphql(active_registry));
    }

    pub fn get_graphql_url(&self) -> String {
        let registry = self.get_current_registry();
        format_graphql(&registry)
    }

    /// Gets the current (active) registry URL
    pub fn get_current_registry(&self) -> String {
        format_graphql(&self.active_registry)
    }

    pub fn current_login(&self) -> Option<&RegistryLogin> {
        self.tokens
            .iter()
            .find(|login| login.registry == self.active_registry)
    }

    /// Sets the current (active) registry URL
    pub fn set_current_registry(&mut self, registry: &str) {
        let registry = format_graphql(registry);
        if let Err(e) = test_if_registry_present(&registry) {
            println!("Error when trying to ping registry {registry:?}: {e}");
            println!("WARNING: Registry {registry:?} will be used, but commands may not succeed.");
        }
        self.active_registry = registry;
    }

    /// Returns the login token for the registry
    pub fn get_login_token_for_registry(&self, registry: &str) -> Option<String> {
        let registry_formatted = format_graphql(registry);
        self.tokens
            .iter()
            .filter(|login| login.registry == registry || login.registry == registry_formatted)
            .last()
            .map(|login| login.token.clone())
    }

    /// Sets the login token for the registry URL
    pub fn set_login_token_for_registry(
        &mut self,
        registry: &str,
        token: &str,
        update_current_registry: UpdateRegistry,
    ) {
        let registry_formatted = format_graphql(registry);
        self.tokens
            .retain(|login| !(login.registry == registry || login.registry == registry_formatted));
        self.tokens.push(RegistryLogin {
            registry: format_graphql(registry),
            token: token.to_string(),
        });
        if update_current_registry == UpdateRegistry::Update {
            self.active_registry = format_graphql(registry);
        }
    }
}

impl WasmerConfig {
    pub(crate) const ENV_VAR_WASMER_REGISTRY_TOKEN: &'static str = "WASMER_TOKEN";
    pub(crate) const ENV_VAR_WASMER_REGISTRY_TOKEN_LEGACY: &'static str = "WAPM_REGISTRY_TOKEN";

    /// Save the config to a file
    pub fn save<P: AsRef<Path>>(&self, to: P) -> anyhow::Result<()> {
        use std::{fs::File, io::Write};
        let config_serialized = toml::to_string(&self)?;
        let mut file = File::create(to)?;
        file.write_all(config_serialized.as_bytes())?;
        Ok(())
    }

    pub fn from_file(wasmer_dir: &Path) -> Result<Self, String> {
        let path = Self::get_file_location(wasmer_dir);
        match std::fs::read_to_string(path) {
            Ok(config_toml) => Ok(toml::from_str(&config_toml).unwrap_or_else(|_| Self::default())),
            Err(_e) => Ok(Self::default()),
        }
    }

    /// Creates and returns the `WASMER_DIR` directory (or $HOME/.wasmer as a fallback)
    pub fn get_wasmer_dir() -> Result<PathBuf, String> {
        Ok(
            if let Some(folder_str) = std::env::var("WASMER_DIR").ok().filter(|s| !s.is_empty()) {
                let folder = PathBuf::from(folder_str);
                std::fs::create_dir_all(folder.clone())
                    .map_err(|e| format!("cannot create config directory: {e}"))?;
                folder
            } else {
                #[allow(unused_variables)]
                let default_dir = Self::get_current_dir()
                    .ok()
                    .unwrap_or_else(|| PathBuf::from("/".to_string()));
                let home_dir =
                    dirs::home_dir().ok_or_else(|| "cannot find home directory".to_string())?;
                let mut folder = home_dir;
                folder.push(".wasmer");
                std::fs::create_dir_all(folder.clone())
                    .map_err(|e| format!("cannot create config directory: {e}"))?;
                folder
            },
        )
    }

    /// Load the config based on environment variables and default config file locations.
    pub fn from_env() -> Result<Self, anyhow::Error> {
        let dir = Self::get_wasmer_dir()
            .map_err(|err| anyhow::anyhow!("Could not determine wasmer dir: {err}"))?;
        let file_path = Self::get_file_location(&dir);
        Self::from_file(&file_path).map_err(|err| {
            anyhow::anyhow!(
                "Could not load config file at '{}': {}",
                file_path.display(),
                err
            )
        })
    }

    pub fn get_current_dir() -> std::io::Result<PathBuf> {
        std::env::current_dir()
    }

    pub fn get_file_location(wasmer_dir: &Path) -> PathBuf {
        wasmer_dir.join(crate::GLOBAL_CONFIG_FILE_NAME)
    }

    pub fn get_database_file_path(wasmer_dir: &Path) -> PathBuf {
        wasmer_dir.join(GLOBAL_CONFIG_DATABASE_FILE_NAME)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registries_switch_token() {
        let mut registries = MultiRegistry::default();

        registries.set_current_registry("https://registry.wasmer.wtf");
        assert_eq!(
            registries.get_current_registry(),
            "https://registry.wasmer.wtf/graphql".to_string()
        );
        registries.set_login_token_for_registry(
            "https://registry.wasmer.io",
            "token1",
            UpdateRegistry::LeaveAsIs,
        );
        assert_eq!(
            registries.get_current_registry(),
            "https://registry.wasmer.wtf/graphql".to_string()
        );
        assert_eq!(
            registries.get_login_token_for_registry(&registries.get_current_registry()),
            None
        );
        registries.set_current_registry("https://registry.wasmer.io");
        assert_eq!(
            registries.get_login_token_for_registry(&registries.get_current_registry()),
            Some("token1".to_string())
        );
        registries.clear_current_registry_token();
        assert_eq!(
            registries.get_login_token_for_registry(&registries.get_current_registry()),
            None
        );
    }

    #[test]
    fn format_registry_urls() {
        let inputs = [
            // Domain names work
            ("wasmer.io", "https://registry.wasmer.io/graphql"),
            ("wasmer.wtf", "https://registry.wasmer.wtf/graphql"),
            // Plain URLs
            (
                "https://registry.wasmer.wtf/graphql",
                "https://registry.wasmer.wtf/graphql",
            ),
            (
                "https://registry.wasmer.wtf/something/else",
                "https://registry.wasmer.wtf/something/else",
            ),
            // We don't automatically prepend the domain name with
            // "registry", but we will make sure "/" gets turned into "/graphql"
            ("https://wasmer.wtf/", "https://wasmer.wtf/graphql"),
            ("https://wasmer.wtf", "https://wasmer.wtf/graphql"),
            // local development
            (
                "http://localhost:8000/graphql",
                "http://localhost:8000/graphql",
            ),
            ("localhost:8000", "http://localhost:8000/graphql"),
        ];

        for (input, expected) in inputs {
            let url = format_graphql(input);
            assert_eq!(url, expected);
        }
    }
}
