use crate::config::UpdateRegistry;
use crate::PartialWapmConfig;

/// Login to a registry and save the token associated with it.
///
/// Also sets the registry as the currently active registry to provide a better UX.
pub fn login_and_save_token(registry: &str, token: &str) -> Result<(), anyhow::Error> {
    let mut config = PartialWapmConfig::from_file().map_err(|e| anyhow::anyhow!("{e}"))?;
    config.registry.set_login_token_for_registry(
        &registry,
        &token,
        UpdateRegistry::Update,
    );
    config.save()?;
    if let Some(s) = crate::utils::get_username().ok().and_then(|o| o) {
        println!("Login for WAPM user {:?} saved", s);
    } else {
        println!("Login for WAPM user saved");
    }
    Ok(())
}
