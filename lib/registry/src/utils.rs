use crate::graphql::execute_query;
use graphql_client::GraphQLQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/queries/whoami.graphql",
    response_derives = "Debug"
)]
struct WhoAmIQuery;

pub fn get_username(registry: &str) -> anyhow::Result<Option<String>> {
    let q = WhoAmIQuery::build_query(who_am_i_query::Variables {});
    let response: who_am_i_query::ResponseData = execute_query(registry, "", &q)?;
    Ok(response.viewer.map(|viewer| viewer.username))
}

pub fn get_username_registry_token(registry: &str, token: &str) -> anyhow::Result<Option<String>> {
    let q = WhoAmIQuery::build_query(who_am_i_query::Variables {});
    let response: who_am_i_query::ResponseData = execute_query(registry, token, &q)?;
    Ok(response.viewer.map(|viewer| viewer.username))
}
