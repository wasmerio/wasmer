use graphql_client::GraphQLQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/mutations/publish_package_chunked.graphql",
    response_derives = "Debug"
)]
pub(crate) struct PublishPackageMutationChunked;
