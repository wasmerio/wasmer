use std::env;

use crate::{
    graphql::subscriptions::{package_version_ready, PackageVersionReady},
    tokio_spawner::TokioSpawner,
};

use futures_util::StreamExt;

use graphql_client::GraphQLQuery;
use graphql_ws_client::{
    graphql::{GraphQLClient, StreamingOperation},
    GraphQLClientClient, SubscriptionStream,
};

use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, http::HeaderValue, Message},
};

async fn subscribe_graphql<Q: GraphQLQuery + Send + Sync + Unpin + 'static>(
    registry_url: &str,
    login_token: &str,
    variables: Q::Variables,
) -> anyhow::Result<(
    SubscriptionStream<GraphQLClient, StreamingOperation<Q>>,
    GraphQLClientClient<Message>,
)>
where
    <Q as GraphQLQuery>::Variables: Send + Sync + Unpin,
{
    let mut url = url::Url::parse(registry_url).unwrap();
    if url.scheme() == "http" {
        url.set_scheme("ws").unwrap();
    } else if url.scheme() == "https" {
        url.set_scheme("wss").unwrap();
    }

    let mut req = url.into_client_request()?;
    req.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_str("graphql-transport-ws")?,
    );
    let token = env::var("WASMER_TOKEN")
        .ok()
        .or_else(|| env::var("WAPM_REGISTRY_TOKEN").ok())
        .unwrap_or_else(|| login_token.to_string());
    req.headers_mut().insert(
        reqwest::header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", token))?,
    );
    let user_agent = crate::client::RegistryClient::default_user_agent();
    req.headers_mut().insert(
        reqwest::header::USER_AGENT,
        HeaderValue::from_str(&user_agent)?,
    );
    let (ws_con, _resp) = connect_async(req).await?;
    let (sink, stream) = ws_con.split();

    let mut client = graphql_ws_client::GraphQLClientClientBuilder::new()
        .build(stream, sink, TokioSpawner::current())
        .await?;
    let stream = client
        .streaming_operation(StreamingOperation::<Q>::new(variables))
        .await?;

    Ok((stream, client))
}

pub async fn subscribe_package_version_ready(
    registry_url: &str,
    login_token: &str,
    package_version_id: &str,
) -> anyhow::Result<(
    SubscriptionStream<GraphQLClient, StreamingOperation<PackageVersionReady>>,
    GraphQLClientClient<Message>,
)> {
    subscribe_graphql(
        registry_url,
        login_token,
        package_version_ready::Variables {
            package_version_id: package_version_id.to_string(),
        },
    )
    .await
}
