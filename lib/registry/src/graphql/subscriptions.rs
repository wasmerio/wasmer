use graphql_client::GraphQLQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/subscriptions/packageVersionReady.graphql",
    response_derives = "Debug"
)]
pub struct PackageVersionReady;
