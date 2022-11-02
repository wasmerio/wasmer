use crate::{graphql::execute_query, PartialWapmConfig};
use graphql_client::GraphQLQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/queries/whoami.graphql",
    response_derives = "Debug"
)]
struct WhoAmIQuery;

pub fn get_username() -> anyhow::Result<Option<String>> {
    let config = PartialWapmConfig::from_file().map_err(|e| anyhow::anyhow!("{e}"))?;
    let registry = &config.registry.get_current_registry();
    let q = WhoAmIQuery::build_query(who_am_i_query::Variables {});
    let response: who_am_i_query::ResponseData = execute_query(registry, "", &q)?;
    Ok(response.viewer.map(|viewer| viewer.username))
}
