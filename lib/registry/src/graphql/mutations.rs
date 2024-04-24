use graphql_client::GraphQLQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/mutations/publish_package_chunked.graphql",
    response_derives = "Debug",
    variables_derives = "Debug"
    // additional_derives = "Debug"
)]
pub(crate) struct PublishPackageMutationChunked;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/mutations/generate_deploy_token.graphql",
    response_derives = "Debug"
)]
pub(crate) struct GenerateDeployToken;

pub type JSONString = String;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/mutations/publish_deploy_app.graphql",
    response_derives = "Debug"
)]
pub(crate) struct PublishDeployApp;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/mutations/new_nonce.graphql",
    response_derives = "Debug"
)]
pub(crate) struct NewNonce;
