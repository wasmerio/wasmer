use std::{env, future::IntoFuture};

use anyhow::Context;

use crate::graphql::subscriptions::{package_version_ready, PackageVersionReady};

use graphql_client::GraphQLQuery;
use graphql_ws_client::{graphql::StreamingOperation, Client, Subscription};

use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue};

pub use crate::graphql::subscriptions::package_version_ready::PackageVersionState;

async fn subscribe_graphql<Q: GraphQLQuery + Send + Sync + Unpin + 'static>(
    registry_url: &str,
    login_token: &str,
    variables: Q::Variables,
) -> anyhow::Result<(Subscription<StreamingOperation<Q>>, Client)>
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
    let (connection, _resp) = async_tungstenite::tokio::connect_async(req)
        .await
        .context("could not connect")?;

    let (client, actor) = graphql_ws_client::Client::build(connection).await?;
    tokio::spawn(actor.into_future());

    let stream = client
        .subscribe(StreamingOperation::<Q>::new(variables))
        .await?;

    Ok((stream, client))
}

pub async fn subscribe_package_version_ready(
    registry_url: &str,
    login_token: &str,
    package_version_id: &str,
) -> anyhow::Result<(
    Subscription<StreamingOperation<PackageVersionReady>>,
    Client,
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
