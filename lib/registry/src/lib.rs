use std::path::{Path, PathBuf};

pub mod graphql {

    use graphql_client::*;
    #[cfg(not(target_os = "wasi"))]
    use reqwest::{
        blocking::{multipart::Form, Client},
        header::USER_AGENT,
    };
    use std::env;
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
            let proxy = if let Ok(proxy_url) = env::var("ALL_PROXY").or(env::var("all_proxy")) {
                reqwest::Proxy::all(&proxy_url).map(|proxy| (proxy_url, proxy, "ALL_PROXY"))
            } else if let Ok(https_proxy_url) = env::var("HTTPS_PROXY").or(env::var("https_proxy"))
            {
                reqwest::Proxy::https(&https_proxy_url)
                    .map(|proxy| (https_proxy_url, proxy, "HTTPS_PROXY"))
            } else if let Ok(http_proxy_url) = env::var("HTTP_PROXY").or(env::var("http_proxy")) {
                reqwest::Proxy::http(&http_proxy_url)
                    .map(|proxy| (http_proxy_url, proxy, "http_proxy"))
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
        query_path = "graphql/queries/get_package.graphql",
        response_derives = "Debug"
    )]
    pub(crate) struct GetPackageQuery;

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

    pub fn execute_query_modifier_inner<R, V, F>(
        registry_url: &str,
        login_token: &str,
        query: &QueryBody<V>,
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

        let res = client
            .post(registry_url)
            .multipart(form)
            .bearer_auth(env::var("WAPM_REGISTRY_TOKEN").unwrap_or(login_token.to_string()))
            .header(USER_AGENT, user_agent)
            .send()?;

        let response_body: Response<R> = res.json()?;
        if let Some(errors) = response_body.errors {
            let error_messages: Vec<String> = errors.into_iter().map(|err| err.message).collect();
            return Err(anyhow::anyhow!("{}", error_messages.join(", ")).into());
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
        execute_query_modifier_inner(registry_url, login_token, query, |f| f)
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord)]
pub struct PackageDownloadInfo {
    pub package: String,
    pub command: String,
    pub url: String,
}

pub fn get_command_local(name: &str) -> Result<PathBuf, String> {
    Err(format!("unimplemented"))
}

pub fn get_package_local(name: &str, version: Option<&str>) -> Result<PathBuf, String> {
    Err(format!("unimplemented"))
}

pub fn query_command_from_registry(name: &str) -> Result<PackageDownloadInfo, String> {
    Err(format!("unimplemented"))
}

pub fn query_package_from_registry(
    name: &str,
    version: Option<&str>,
) -> Result<PackageDownloadInfo, String> {
    Err(format!("unimplemented"))
}

/// Returs the path to the directory where all packages on this computer are being stored
pub fn get_global_install_dir(registry_host: &str) -> Option<PathBuf> {
    Some(
        dirs::home_dir()?
            .join(".wasmer")
            .join("checkouts")
            .join(registry_host),
    )
}

pub fn download_and_unpack_targz(url: &str, target_path: &Path) -> Result<PathBuf, String> {
    Err(format!("unimplemented"))
}

pub fn install_package(name: &str, version: Option<&str>) -> Result<PathBuf, String> {
    std::thread::sleep(std::time::Duration::from_secs(4));
    Err(format!("unimplemented"))
}

pub fn test_if_registry_present(url: &str) -> Result<bool, String> {
    Ok(false)
}

pub fn get_all_available_registries() -> Result<Vec<String>, String> {
    Ok(Vec::new())
}
