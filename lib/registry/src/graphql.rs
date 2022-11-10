use graphql_client::*;
#[cfg(not(target_os = "wasi"))]
use reqwest::{
    blocking::{multipart::Form, Client},
    header::USER_AGENT,
};
use std::env;
use std::time::Duration;
#[cfg(target_os = "wasi")]
use {wasm_bus_reqwest::prelude::header::*, wasm_bus_reqwest::prelude::*};

mod proxy {
    //! Code for dealing with setting things up to proxy network requests
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum ProxyError {
        #[error("Failed to parse URL from {}: {}", url_location, error_message)]
        UrlParseError {
            url_location: String,
            error_message: String,
        },

        #[error("Could not connect to proxy: {0}")]
        ConnectionError(String),
    }

    /// Tries to set up a proxy
    ///
    /// This function reads from wapm config's `proxy.url` first, then checks
    /// `ALL_PROXY`, `HTTPS_PROXY`, and `HTTP_PROXY` environment variables, in both
    /// upper case and lower case, in that order.
    ///
    /// If a proxy is specified in wapm config's `proxy.url`, it is assumed
    /// to be a general proxy
    ///
    /// A return value of `Ok(None)` means that there was no attempt to set up a proxy,
    /// `Ok(Some(proxy))` means that the proxy was set up successfully, and `Err(e)` that
    /// there was a failure while attempting to set up the proxy.
    pub fn maybe_set_up_proxy() -> anyhow::Result<Option<reqwest::Proxy>> {
        use std::env;
        let proxy = if let Ok(proxy_url) = env::var("ALL_PROXY").or_else(|_| env::var("all_proxy"))
        {
            reqwest::Proxy::all(&proxy_url).map(|proxy| (proxy_url, proxy, "ALL_PROXY"))
        } else if let Ok(https_proxy_url) =
            env::var("HTTPS_PROXY").or_else(|_| env::var("https_proxy"))
        {
            reqwest::Proxy::https(&https_proxy_url)
                .map(|proxy| (https_proxy_url, proxy, "HTTPS_PROXY"))
        } else if let Ok(http_proxy_url) =
            env::var("HTTP_PROXY").or_else(|_| env::var("http_proxy"))
        {
            reqwest::Proxy::http(&http_proxy_url).map(|proxy| (http_proxy_url, proxy, "http_proxy"))
        } else {
            return Ok(None);
        }
        .map_err(|e| ProxyError::ConnectionError(e.to_string()))
        .and_then(
            |(proxy_url_str, proxy, url_location): (String, _, &'static str)| {
                url::Url::parse(&proxy_url_str)
                    .map_err(|e| ProxyError::UrlParseError {
                        url_location: url_location.to_string(),
                        error_message: e.to_string(),
                    })
                    .map(|url| {
                        if !(url.username().is_empty()) && url.password().is_some() {
                            proxy.basic_auth(url.username(), url.password().unwrap_or_default())
                        } else {
                            proxy
                        }
                    })
            },
        )?;

        Ok(Some(proxy))
    }
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/queries/get_package_version.graphql",
    response_derives = "Debug"
)]
pub(crate) struct GetPackageVersionQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/queries/get_package_by_command.graphql",
    response_derives = "Debug"
)]
pub(crate) struct GetPackageByCommandQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/queries/test_if_registry_present.graphql",
    response_derives = "Debug"
)]
pub(crate) struct TestIfRegistryPresent;

#[cfg(target_os = "wasi")]
pub fn whoami_distro() -> String {
    whoami::os().to_lowercase()
}

#[cfg(not(target_os = "wasi"))]
pub fn whoami_distro() -> String {
    whoami::distro().to_lowercase()
}

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
    let client = {
        let builder = Client::builder();

        #[cfg(not(target_os = "wasi"))]
        let builder = if let Some(proxy) = proxy::maybe_set_up_proxy()? {
            builder.proxy(proxy)
        } else {
            builder
        };
        builder.build()?
    };

    let vars = serde_json::to_string(&query.variables).unwrap();

    let form = Form::new()
        .text("query", query.query.to_string())
        .text("operationName", query.operation_name.to_string())
        .text("variables", vars);

    let form = form_modifier(form);

    let user_agent = format!(
        "wapm/{} {} {}",
        env!("CARGO_PKG_VERSION"),
        whoami::platform(),
        whoami_distro(),
    );

    let mut res = client
        .post(registry_url)
        .multipart(form)
        .bearer_auth(env::var("WAPM_REGISTRY_TOKEN").unwrap_or_else(|_| login_token.to_string()))
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
    let client = {
        let builder = Client::builder();

        #[cfg(not(target_os = "wasi"))]
        let builder = if let Some(proxy) = proxy::maybe_set_up_proxy()? {
            builder.proxy(proxy)
        } else {
            builder
        };
        builder.build()?
    };

    let vars = serde_json::to_string(&query.variables).unwrap();

    let form = Form::new()
        .text("query", query.query.to_string())
        .text("operationName", query.operation_name.to_string())
        .text("variables", vars);

    let form = form_modifier(form);

    let user_agent = format!(
        "wapm/{} {} {}",
        env!("CARGO_PKG_VERSION"),
        whoami::platform(),
        whoami_distro(),
    );

    let mut res = client
        .post(registry_url)
        .multipart(form)
        .bearer_auth(env::var("WAPM_REGISTRY_TOKEN").unwrap_or_else(|_| login_token.to_string()))
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
    Ok(response_body.data.expect("missing response data"))
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
