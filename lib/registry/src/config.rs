use graphql_client::GraphQLQuery;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Default, Serialize, Debug, PartialEq, Eq)]
pub struct PartialWapmConfig {
    /// The number of seconds to wait before checking the registry for a new
    /// version of the package.
    #[serde(default = "wax_default_cooldown")]
    pub wax_cooldown: i32,

    /// The registry that wapm will connect to.
    pub registry: Registries,

    /// Whether or not telemetry is enabled.
    #[serde(default)]
    pub telemetry: Telemetry,

    /// Whether or not updated notifications are enabled.
    #[serde(default)]
    pub update_notifications: UpdateNotifications,

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

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq, Default)]
pub struct UpdateNotifications {
    pub enabled: String,
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq, Default)]
pub struct Telemetry {
    pub enabled: String,
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq, Clone)]
#[serde(untagged)]
pub enum Registries {
    Single(Registry),
    Multi(MultiRegistry),
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq, Clone)]
pub struct MultiRegistry {
    /// Currently active registry
    pub current: String,
    /// Map from "RegistryUrl" to "LoginToken", in order to
    /// be able to be able to easily switch between registries
    pub tokens: BTreeMap<String, String>,
}

impl Default for Registries {
    fn default() -> Self {
        Registries::Single(Registry {
            url: format_graphql("https://registry.wapm.io"),
            token: None,
        })
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq, Clone)]
pub struct Registry {
    pub url: String,
    pub token: Option<String>,
}

pub fn format_graphql(registry: &str) -> String {
    let mut registry = registry.to_string();
    if registry.contains("wapm.dev") {
        registry = "https://registry.wapm.dev/graphql".to_string();
    } else if registry.contains("wapm.io") {
        registry = "https://registry.wapm.io/graphql".to_string();
    }
    if !registry.starts_with("https://") {
        registry = format!("https://{registry}");
    }
    if registry.ends_with("/graphql") {
        registry
    } else if registry.ends_with('/') {
        format!("{}graphql", registry)
    } else {
        format!("{}/graphql", registry)
    }
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

#[test]
fn test_registries_switch_token() {
    let mut registries = Registries::default();

    registries.set_current_registry("https://registry.wapm.dev");
    assert_eq!(
        registries.get_current_registry(),
        "https://registry.wapm.dev/graphql".to_string()
    );
    registries.set_login_token_for_registry(
        "https://registry.wapm.io",
        "token1",
        UpdateRegistry::LeaveAsIs,
    );
    assert_eq!(
        registries.get_current_registry(),
        "https://registry.wapm.dev/graphql".to_string()
    );
    assert_eq!(
        registries.get_login_token_for_registry(&registries.get_current_registry()),
        None
    );
    registries.set_current_registry("https://registry.wapm.io");
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

impl Registries {
    /// Gets the current (active) registry URL
    pub fn clear_current_registry_token(&mut self) {
        match self {
            Registries::Single(s) => {
                s.token = None;
            }
            Registries::Multi(m) => {
                m.tokens.remove(&m.current);
                m.tokens.remove(&format_graphql(&m.current));
            }
        }
    }

    pub fn get_graphql_url(&self) -> String {
        let registry = self.get_current_registry();
        format_graphql(&registry)
    }

    /// Gets the current (active) registry URL
    pub fn get_current_registry(&self) -> String {
        match self {
            Registries::Single(s) => format_graphql(&s.url),
            Registries::Multi(m) => format_graphql(&m.current),
        }
    }

    /// Sets the current (active) registry URL
    pub fn set_current_registry(&mut self, registry: &str) {
        let registry = format_graphql(registry);
        if let Err(e) = test_if_registry_present(&registry) {
            println!("Error when trying to ping registry {registry:?}: {e}");
            println!("WARNING: Registry {registry:?} will be used, but commands may not succeed.");
        }
        match self {
            Registries::Single(s) => s.url = registry,
            Registries::Multi(m) => m.current = registry,
        }
    }

    /// Returns the login token for the registry
    pub fn get_login_token_for_registry(&self, registry: &str) -> Option<String> {
        match self {
            Registries::Single(s) if s.url == registry || format_graphql(registry) == s.url => {
                s.token.clone()
            }
            Registries::Multi(m) => m
                .tokens
                .get(registry)
                .or_else(|| m.tokens.get(&format_graphql(registry)))
                .cloned(),
            _ => None,
        }
    }

    /// Sets the login token for the registry URL
    pub fn set_login_token_for_registry(
        &mut self,
        registry: &str,
        token: &str,
        update_current_registry: UpdateRegistry,
    ) {
        let new_map = match self {
            Registries::Single(s) => {
                if s.url == registry {
                    Registries::Single(Registry {
                        url: format_graphql(registry),
                        token: Some(token.to_string()),
                    })
                } else {
                    let mut map = BTreeMap::new();
                    if let Some(token) = s.token.clone() {
                        map.insert(format_graphql(&s.url), token);
                    }
                    map.insert(format_graphql(registry), token.to_string());
                    Registries::Multi(MultiRegistry {
                        current: format_graphql(&s.url),
                        tokens: map,
                    })
                }
            }
            Registries::Multi(m) => {
                m.tokens.insert(format_graphql(registry), token.to_string());
                if update_current_registry == UpdateRegistry::Update {
                    m.current = format_graphql(registry);
                }
                Registries::Multi(m.clone())
            }
        };
        *self = new_map;
    }
}

impl PartialWapmConfig {
    /// Save the config to a file
    pub fn save<P: AsRef<Path>>(&self, to: P) -> anyhow::Result<()> {
        use std::{fs::File, io::Write};
        let config_serialized = toml::to_string(&self)?;
        let mut file = File::create(to)?;
        file.write_all(config_serialized.as_bytes())?;
        Ok(())
    }

    pub fn from_file(#[cfg(test)] test_name: &str) -> Result<Self, String> {
        #[cfg(test)]
        let path = Self::get_file_location(test_name)?;
        #[cfg(not(test))]
        let path = Self::get_file_location()?;

        match std::fs::read_to_string(&path) {
            Ok(config_toml) => {
                toml::from_str(&config_toml).map_err(|e| format!("could not parse {path:?}: {e}"))
            }
            Err(_e) => Ok(Self::default()),
        }
    }

    pub fn get_current_dir() -> std::io::Result<PathBuf> {
        std::env::current_dir()
    }

    #[cfg(test)]
    pub fn get_folder(test_name: &str) -> Result<PathBuf, String> {
        let test_dir = std::env::temp_dir().join("test_wasmer").join(test_name);
        let _ = std::fs::create_dir_all(&test_dir);
        Ok(test_dir)
    }

    #[cfg(not(test))]
    pub fn get_folder() -> Result<PathBuf, String> {
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

    #[cfg(test)]
    pub fn get_file_location(test_name: &str) -> Result<PathBuf, String> {
        Ok(Self::get_folder(test_name)?.join(crate::GLOBAL_CONFIG_FILE_NAME))
    }

    #[cfg(not(test))]
    pub fn get_file_location() -> Result<PathBuf, String> {
        Ok(Self::get_folder()?.join(crate::GLOBAL_CONFIG_FILE_NAME))
    }
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/queries/test_if_registry_present.graphql",
    response_derives = "Debug"
)]
struct TestIfRegistryPresent;
