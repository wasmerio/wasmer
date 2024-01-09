pub(crate) mod mutations;
pub(crate) mod proxy;
pub(crate) mod queries;
pub(crate) mod subscriptions;

use graphql_client::*;
use reqwest::{
    blocking::{multipart::Form, Client},
    header::USER_AGENT,
};
use std::env;
use std::time::Duration;

use crate::config::WasmerConfig;

pub fn whoami_distro() -> String {
    #[cfg(target_os = "wasi")]
    {
        whoami::os().to_lowercase()
    }
    #[cfg(not(target_os = "wasi"))]
    {
        whoami::distro().to_lowercase()
    }
}

fn setup_client() -> Result<Client, anyhow::Error> {
    let builder = Client::builder();
    let builder = proxy::maybe_set_up_proxy_blocking(builder)?;
    builder.build().map_err(|e| e.into())
}

/// This function is being used to "ping" the registry
/// (see test_if_registry_present and see whether the response
/// is valid JSON, it doesn't check the response itself,
/// since the response format might change
pub fn execute_query_modifier_inner_check_json<V, F>(
    registry_url: &str,
    login_token: &str,
    query: &QueryBody<V>,
    timeout: Option<Duration>,
    form_modifier: F,
) -> anyhow::Result<()>
where
    V: serde::Serialize,
    F: FnOnce(Form) -> Form,
{
    let client = setup_client()?;

    let vars = serde_json::to_string(&query.variables).unwrap();

    let form = Form::new()
        .text("query", query.query.to_string())
        .text("operationName", query.operation_name.to_string())
        .text("variables", vars);

    let form = form_modifier(form);

    let user_agent = crate::client::RegistryClient::default_user_agent();

    let mut res = client
        .post(registry_url)
        .multipart(form)
        .bearer_auth(
            env::var(WasmerConfig::ENV_VAR_WASMER_REGISTRY_TOKEN)
                .ok()
                .or_else(|| env::var(WasmerConfig::ENV_VAR_WASMER_REGISTRY_TOKEN_LEGACY).ok())
                .unwrap_or_else(|| login_token.to_string()),
        )
        .header(USER_AGENT, user_agent);

    if let Some(t) = timeout {
        res = res.timeout(t);
    }

    let res = res.send()?;

    let _: Response<serde_json::Value> = res.json()?;

    Ok(())
}

pub fn execute_query_modifier_inner<R, V, F>(
    registry_url: &str,
    login_token: &str,
    query: &QueryBody<V>,
    timeout: Option<Duration>,
    form_modifier: F,
) -> anyhow::Result<R>
where
    for<'de> R: serde::Deserialize<'de>,
    V: serde::Serialize,
    F: FnOnce(Form) -> Form,
{
    let client = setup_client()?;

    let vars = serde_json::to_string(&query.variables).unwrap();

    let form = Form::new()
        .text("query", query.query.to_string())
        .text("operationName", query.operation_name.to_string())
        .text("variables", vars);

    let form = form_modifier(form);

    let user_agent = crate::client::RegistryClient::default_user_agent();

    let mut res = client
        .post(registry_url)
        .multipart(form)
        .bearer_auth(
            env::var("WASMER_TOKEN")
                .ok()
                .or_else(|| env::var("WAPM_REGISTRY_TOKEN").ok())
                .unwrap_or_else(|| login_token.to_string()),
        )
        .header(USER_AGENT, user_agent);

    if let Some(t) = timeout {
        res = res.timeout(t);
    }

    let res = res.send()?;
    let response_body: Response<R> = res.json()?;
    if let Some(errors) = response_body.errors {
        let error_messages: Vec<String> = errors.into_iter().map(|err| err.message).collect();
        return Err(anyhow::anyhow!("{}", error_messages.join(", ")));
    }
    response_body
        .data
        .ok_or_else(|| anyhow::anyhow!("missing response data"))
}

pub fn execute_query<R, V>(
    registry_url: &str,
    login_token: &str,
    query: &QueryBody<V>,
) -> anyhow::Result<R>
where
    for<'de> R: serde::Deserialize<'de>,
    V: serde::Serialize,
{
    execute_query_modifier_inner(registry_url, login_token, query, None, |f| f)
}

pub fn execute_query_with_timeout<R, V>(
    registry_url: &str,
    login_token: &str,
    timeout: Duration,
    query: &QueryBody<V>,
) -> anyhow::Result<R>
where
    for<'de> R: serde::Deserialize<'de>,
    V: serde::Serialize,
{
    execute_query_modifier_inner(registry_url, login_token, query, Some(timeout), |f| f)
}
