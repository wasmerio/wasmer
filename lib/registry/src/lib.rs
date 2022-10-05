use std::path::{Path, PathBuf};
use std::env;
use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;

pub mod graphql {

    use graphql_client::*;
    #[cfg(not(target_os = "wasi"))]
    use reqwest::{
        blocking::{multipart::Form, Client},
        header::USER_AGENT,
    };
    use core::time;
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
        query_path = "graphql/queries/get_packages.graphql",
        response_derives = "Debug"
    )]
    pub(crate) struct GetPackagesQuery;

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
            .bearer_auth(env::var("WAPM_REGISTRY_TOKEN").unwrap_or(login_token.to_string()))
            .header(USER_AGENT, user_agent);
        
        if let Some(t) = timeout {
            res = res.timeout(t);
        }

        let res = res.send()?;

        let _: Response<serde_json::Value> = res.json()?;
        return Ok(());
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
            .bearer_auth(env::var("WAPM_REGISTRY_TOKEN").unwrap_or(login_token.to_string()))
            .header(USER_AGENT, user_agent);
        
        if let Some(t) = timeout {
            res = res.timeout(t);
        }

        let res = res.send()?;
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
        execute_query_modifier_inner(registry_url, login_token, query, None, |f| f)
    }

    pub fn execute_query_with_timeout<R, V>(
        registry_url: &str,
        login_token: &str,
        timeout: Duration,
        only_check_json_response: bool,
        query: &QueryBody<V>,
    ) -> anyhow::Result<R>
    where
        for<'de> R: serde::Deserialize<'de>,
        V: serde::Serialize,
    {
        execute_query_modifier_inner(
            registry_url, 
            login_token, 
            query, 
            Some(timeout), 
            |f| f
        )
    }
}

pub static GLOBAL_CONFIG_FILE_NAME: &str = if cfg!(target_os = "wasi") {
    "/.private/wapm.toml"
} else {
    "wapm.toml"
};

#[derive(Deserialize, Default, Serialize, Debug, PartialEq)]
pub struct PartialWapmConfig {
    /// The number of seconds to wait before checking the registry for a new
    /// version of the package.
    #[serde(default = "wax_default_cooldown")]
    pub wax_cooldown: i32,

    /// The registry that wapm will connect to.
    pub registry: Registries,

    /// Whether or not telemetry is enabled.
    #[cfg(feature = "telemetry")]
    #[serde(default)]
    pub telemetry: Telemetry,

    /// Whether or not updated notifications are enabled.
    #[cfg(feature = "update-notifications")]
    #[serde(default)]
    pub update_notifications: UpdateNotifications,

    /// The proxy to use when connecting to the Internet.
    #[serde(default)]
    pub proxy: Proxy,
}

pub const fn wax_default_cooldown() -> i32 {
    5 * 60
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Default)]
pub struct Proxy {
    pub url: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Default)]
pub struct UpdateNotifications {
    pub enabled: String,
}

#[cfg(feature = "telemetry")]
#[derive(Deserialize, Serialize, Debug, PartialEq)]
pub struct Telemetry {
    pub enabled: String,
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Clone)]
#[serde(untagged)]
pub enum Registries {
    Single(Registry),
    Multi(MultiRegistry),
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Clone)]
pub struct MultiRegistry {
    /// Currently active registry
    pub current: String,
    /// Map from "RegistryUrl" to "LoginToken", in order to
    /// be able to be able to easily switch between registries
    pub tokens: BTreeMap<String, String>,
}

impl Default for Registries {
    fn default() -> Self {
        Registries::Single(Registry {
            url: format_graphql("https://registry.wapm.io"),
            token: None,
        })
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Clone)]
pub struct Registry {
    pub url: String,
    pub token: Option<String>,
}

fn format_graphql(registry: &str) -> String {
    if registry.ends_with("/graphql") {
        registry.to_string()
    } else if registry.ends_with("/") {
        format!("{}graphql", registry)
    } else {
        format!("{}/graphql", registry)
    }
}

impl PartialWapmConfig {

    pub fn from_file() -> Result<Self, String> {
        let path = Self::get_file_location()?;

        match std::fs::read_to_string(&path) {
            Ok(config_toml) => {
                toml::from_str(&config_toml)
                .map_err(|e| format!("could not parse {path:?}: {e}"))
            }
            Err(_e) => Ok(Self::default()),
        }
    }

    pub fn get_current_dir() -> std::io::Result<PathBuf> {
        #[cfg(target_os = "wasi")]
        if let Some(pwd) = std::env::var("PWD").ok() {
            return Ok(PathBuf::from(pwd));
        }
        Ok(std::env::current_dir()?)
    }

    pub fn get_folder() -> Result<PathBuf, String> {
        Ok(
            if let Some(folder_str) = env::var("WASMER_DIR")
                .ok()
                .filter(|s| !s.is_empty())
            {
                let folder = PathBuf::from(folder_str);
                std::fs::create_dir_all(folder.clone())
                    .map_err(|e| format!("cannot create config directory: {e}"))?;
                folder
            } else {
                #[allow(unused_variables)]
                let default_dir = Self::get_current_dir()
                    .ok()
                    .unwrap_or_else(|| PathBuf::from("/".to_string()));
                #[cfg(feature = "dirs")]
                let home_dir =
                    dirs::home_dir().ok_or(GlobalConfigError::CannotFindHomeDirectory)?;
                #[cfg(not(feature = "dirs"))]
                let home_dir = std::env::var("HOME")
                    .ok()
                    .unwrap_or_else(|| default_dir.to_string_lossy().to_string());
                let mut folder = PathBuf::from(home_dir);
                folder.push(".wasmer");
                std::fs::create_dir_all(folder.clone())
                    .map_err(|e| format!("cannot create config directory: {e}"))?;
                folder
            },
        )
    }

    fn get_file_location() -> Result<PathBuf, String> {
        Ok(Self::get_folder()?.join(GLOBAL_CONFIG_FILE_NAME))
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord)]
pub struct PackageDownloadInfo {
    pub registry: String,
    pub package: String,
    pub version: String,
    pub commands: String,
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

/// Returns the download info of the packages, on error returns all the available packages
/// i.e. (("foo/python", "wapm.io"), ("bar/python" "wapm.io")))
pub fn query_package_from_registry(
    registry_url: &str,
    name: &str,
    version: Option<&str>,
) -> Result<PackageDownloadInfo, (Vec<PackageDownloadInfo>, String)> {

    use crate::graphql::{
        GetPackagesQuery, get_packages_query,
        execute_query
    };
    use graphql_client::GraphQLQuery;

    let q = if name.contains("/") {
        let name = name.split("/").nth(1).unwrap();
        GetPackagesQuery::build_query(get_packages_query::Variables {
            names: vec![name.to_string()],
        })
    } else {
        GetPackagesQuery::build_query(get_packages_query::Variables {
            names: vec![name.to_string()],
        })
    };

    let response: get_packages_query::ResponseData = execute_query(registry_url, "", &q)
    .map_err(|e| (Vec::new(), format!("Error sending GetPackagesQuery:  {e}")))?;

    let available_packages = response.package
    .iter()
    .filter_map(|p| {
        
        let p = p.as_ref()?;
        let mut versions = Vec::new();
        
        for v in p.versions.iter() {
            for v in v.iter() {
                let v = match v.as_ref() {
                    Some(s) => s,
                    None => continue,
                };

                versions.push(PackageDownloadInfo { 
                    registry: registry_url.to_string(),
                    package: p.name.clone(), 
    
                    version: v.version.clone(),
    
                    commands: toml::from_str::<wapm_toml::Manifest>(
                        &v.manifest
                    ).ok()?
                    .command.unwrap_or_default()
                    .iter().map(|s| s.get_name()).collect(), 
                    
                    url: v.distribution.download_url.clone(), 
                });
            }
        }

        Some(versions)
    })
    .collect::<Vec<_>>()
    .into_iter()
    .flat_map(|v| v.into_iter())
    .collect::<Vec<_>>();

    let mut queried_package = available_packages.iter()
    .filter_map(|v| {
        if name.contains("/") && v.package != name {
            return None;
        }

        if version.is_some() && v.version != version.clone().unwrap() {
            return None;
        }

        Some(v)
    }).next().cloned();

    match queried_package {
        None => Err((available_packages, format!("No package found for {name}@{}", version.unwrap_or("latest")))),
        Some(s) => Ok(s),
    }
}

/// Returs the path to the directory where all packages on this computer are being stored
pub fn get_global_install_dir(registry_host: &str) -> Option<PathBuf> {
    Some(
        PartialWapmConfig::get_folder().ok()?
            .join("checkouts")
            .join(registry_host),
    )
}

pub fn download_and_unpack_targz(url: &str, target_path: &Path) -> Result<PathBuf, String> {
    Err(format!("unimplemented"))
}

pub fn install_package(name: &str, version: Option<&str>) -> Result<PathBuf, String> {
    let registries = get_all_available_registries()?;
    let mut url_of_package = None;
    let mut error_packages = Vec::new();
    for r in registries.iter() {
        let registry_test = test_if_registry_present(r);
        if !registry_test.clone().unwrap_or(false) {
            println!(" warning: registry {r} not present: {:#?}", registry_test);
            continue;
        }
        match query_package_from_registry(&r, name, version) {
            Ok(o) => {
                url_of_package = Some((r, o));
                break;
            },
            Err(e) => {
                error_packages.push(e);
            },
        }
    }
    
    let version_str = match version {
        None => format!("{name}"),
        Some(v) => format!("{name}@{v}"),
    };
    
    let registries_searched = registries
        .iter()
        .filter_map(|s| url::Url::parse(s).ok())
        .filter_map(|s| Some(format!("{}", s.host_str()?)))
        .collect::<Vec<_>>();

    let did_you_mean = error_packages.iter()
    .flat_map(|(packages, _)| {

    }).collect::<Vec<_>>();
    
    // println!("error packages: {:#?}", error_packages);

    let url_of_package = url_of_package
    .ok_or(format!("Package {version_str} not found in registries: {registries_searched:#?}"))?;

    println!("url of package: {:#?} in registries: {registries_searched:#?}", url_of_package.1);
    Err(format!("unimplemented"))
}

pub fn test_if_registry_present(registry: &str) -> Result<bool, String> {

    use graphql_client::GraphQLQuery;
    use std::time::Duration;
    use crate::graphql::{TestIfRegistryPresent, test_if_registry_present};

    let q = TestIfRegistryPresent::build_query(test_if_registry_present::Variables {});
    let _ = crate::graphql::execute_query_modifier_inner_check_json(
        registry, 
        "", 
        &q, 
        Some(Duration::from_secs(1)), 
        |f| f
    ).map_err(|e| format!("{e}"))?;

    Ok(true)
}

pub fn get_all_available_registries() -> Result<Vec<String>, String> {
    let config = PartialWapmConfig::from_file()?;
    let mut registries = Vec::new();
    match config.registry {
        Registries::Single(s) => {
            registries.push(format_graphql(&s.url));
        },
        Registries::Multi(m) => {
            for key in m.tokens.keys() {
                registries.push(format_graphql(&key));
            }
        }   
    }
    Ok(registries)
}
