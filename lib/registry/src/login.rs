use crate::config::{format_graphql, UpdateRegistry};
use crate::WasmerConfig;
use std::path::Path;

/// Login to a registry and save the token associated with it.
///
/// Also sets the registry as the currently active registry to provide a better UX.
pub fn login_and_save_token(
    wasmer_dir: &Path,
    registry: &str,
    token: &str,
) -> Result<Option<String>, anyhow::Error> {
    let registry = format_graphql(registry);
    let mut config = WasmerConfig::from_file(wasmer_dir)
        .map_err(|e| anyhow::anyhow!("config from file: {e}"))?;
    config.registry.set_current_registry(&registry);
    config.registry.set_login_token_for_registry(
        &config.registry.get_current_registry(),
        token,
        UpdateRegistry::Update,
    );
    let path = WasmerConfig::get_file_location(wasmer_dir);
    config.save(path)?;
    crate::utils::get_username_registry_token(&registry, token)
}
