use crate::graphql::{
    execute_query,
    queries::{who_am_i_query, WhoAmIQuery},
};
use graphql_client::GraphQLQuery;

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

pub fn normalize_path(s: &str) -> String {
    s.strip_prefix(r"\\?\").unwrap_or(s).to_string()
}
