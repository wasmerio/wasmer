use crate::config::{format_graphql, Registries, UpdateRegistry};
use crate::PartialWapmConfig;

/// Login to a registry and save the token associated with it.
///
/// Also sets the registry as the currently active registry to provide a better UX.
pub fn login_and_save_token(registry: &str, token: &str) -> Result<(), anyhow::Error> {
    let registry = format_graphql(registry);
    let mut config = PartialWapmConfig::from_file().map_err(|e| anyhow::anyhow!("{e}"))?;
    config.registry.set_current_registry(&registry);
    config.registry.set_login_token_for_registry(
        &config.registry.get_current_registry(),
        &token,
        UpdateRegistry::Update,
    );
    config.save()?;
    let username = crate::utils::get_username_registry_token(&registry, token);
    if let Some(s) = username.ok().and_then(|o| o) {
        println!("Login for WAPM user {:?} saved", s);
    } else {
        println!("Login for WAPM user saved");
    }
    Ok(())
}
