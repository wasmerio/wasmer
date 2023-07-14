use std::path::Path;

use graphql_client::GraphQLQuery;

use crate::{
    config::{format_graphql, UpdateRegistry},
    graphql::{
        execute_query,
        queries::{who_am_i_query, WhoAmIQuery},
    },
    CurrentUser, WasmerConfig,
};

/// Login to a registry and save the token associated with it.
///
/// Also sets the registry as the currently active registry to provide a better UX.
pub fn login_and_save_token(
    wasmer_dir: &Path,
    registry: &str,
    token: &str,
) -> Result<Option<CurrentUser>, anyhow::Error> {
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

    let q = WhoAmIQuery::build_query(who_am_i_query::Variables {});
    let response: who_am_i_query::ResponseData = execute_query(&registry, "", &q)?;

    match response.viewer {
        Some(viewer) => Ok(Some(CurrentUser {
            registry,
            user: viewer.username,
            verified: viewer.is_email_validated,
        })),
        None => Ok(None),
    }
}
