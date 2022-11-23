//! The GraphQL queries used by this crate.
//!
//! This module can be useful if you want to query the WAPM backend directly
//! because the crate's high-level functions don't provide the exact
//! operations/data you need.

use graphql_client::*;

/// The GraphQL schema exposed by the WAPM backend.
pub const SCHEMA: &str = include_str!("../graphql/schema.graphql");

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/queries/get_package_version.graphql",
    response_derives = "Debug"
)]
pub struct GetPackageVersionQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/queries/whoami.graphql",
    response_derives = "Debug"
)]
pub struct WhoAmIQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/queries/get_package_by_command.graphql",
    response_derives = "Debug"
)]
pub struct GetPackageByCommandQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/queries/test_if_registry_present.graphql",
    response_derives = "Debug"
)]
pub struct TestIfRegistryPresent;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/queries/get_bindings.graphql",
    response_derives = "Debug,Clone,PartialEq,Eq"
)]
pub struct GetBindingsQuery;
