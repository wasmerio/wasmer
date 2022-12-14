use crate::{graphql::execute_query, PartialWapmConfig};
use graphql_client::GraphQLQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/queries/whoami.graphql",
    response_derives = "Debug"
)]
struct WhoAmIQuery;

pub fn get_username(#[cfg(test)] test_name: &str) -> anyhow::Result<Option<String>> {
    #[cfg(test)]
    let config = PartialWapmConfig::from_file(test_name).map_err(|e| anyhow::anyhow!("{e}"))?;
    #[cfg(not(test))]
    let config = PartialWapmConfig::from_file().map_err(|e| anyhow::anyhow!("{e}"))?;
    let registry = &config.registry.get_current_registry();
    let q = WhoAmIQuery::build_query(who_am_i_query::Variables {});
    let response: who_am_i_query::ResponseData = execute_query(registry, "", &q)?;
    Ok(response.viewer.map(|viewer| viewer.username))
}

pub fn get_username_registry_token(registry: &str, token: &str) -> anyhow::Result<Option<String>> {
    let q = WhoAmIQuery::build_query(who_am_i_query::Variables {});
    let response: who_am_i_query::ResponseData = execute_query(registry, token, &q)?;
    Ok(response.viewer.map(|viewer| viewer.username))
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/queries/get_namespaces_for_user.graphql",
    response_derives = "Debug"
)]
struct GetNamespacesForUser;

pub fn user_can_publish_under_namespace(
    registry: &str,
    username: &str,
    namespace: &str,
) -> anyhow::Result<bool> {
    Ok(get_namespaces_for_user(registry, username)?
        .iter()
        .any(|s| s == namespace))
}

pub fn get_namespaces_for_user(registry: &str, username: &str) -> anyhow::Result<Vec<String>> {
    let q = GetNamespacesForUser::build_query(get_namespaces_for_user::Variables {
        username: username.to_string(),
    });
    let response: get_namespaces_for_user::ResponseData = execute_query(registry, "", &q)?;
    let user = response.user.ok_or_else(|| {
        anyhow::anyhow!("username {username:?} not found in registry {registry:?}")
    })?;

    Ok(user
        .namespaces
        .edges
        .into_iter()
        .filter_map(|f| Some(f?.node?.global_name.to_string()))
        .collect())
}
