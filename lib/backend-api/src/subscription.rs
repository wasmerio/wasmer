use crate::{
    types::{PackageVersionReadySubscription, PackageVersionReadySubscriptionVariables},
    WasmerClient,
};
use anyhow::Context;
use async_tungstenite::tungstenite::client::IntoClientRequest;
use cynic::SubscriptionBuilder;
use graphql_ws_client::Subscription;
use reqwest::header::HeaderValue;
use std::future::IntoFuture;

pub async fn package_version_ready(
    client: &WasmerClient,
    package_version_id: &str,
) -> anyhow::Result<
    Subscription<
        cynic::StreamingOperation<
            PackageVersionReadySubscription,
            PackageVersionReadySubscriptionVariables,
        >,
    >,
> {
    let mut url = client.graphql_endpoint().clone();
    if url.scheme() == "http" {
        url.set_scheme("ws").unwrap();
    } else if url.scheme() == "https" {
        url.set_scheme("wss").unwrap();
    }

    let url = url.to_string();
    let mut req = url.into_client_request()?;

    req.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_str("graphql-transport-ws").unwrap(),
    );

    if let Some(token) = client.auth_token() {
        req.headers_mut().insert(
            reqwest::header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))?,
        );
    }

    req.headers_mut()
        .insert(reqwest::header::USER_AGENT, client.user_agent.clone());

    let (connection, _resp) = async_tungstenite::tokio::connect_async(req)
        .await
        .context("could not connect")?;

    let (client, actor) = graphql_ws_client::Client::build(connection).await?;
    tokio::spawn(actor.into_future());

    let stream = client
        .subscribe(PackageVersionReadySubscription::build(
            PackageVersionReadySubscriptionVariables {
                package_version_id: cynic::Id::new(package_version_id),
            },
        ))
        .await?;

    Ok(stream)
}
