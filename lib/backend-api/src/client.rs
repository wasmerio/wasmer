#[cfg(not(target_family = "wasm"))]
use std::time::Duration;

use crate::GraphQLApiFailure;
use anyhow::{Context as _, bail};
use cynic::{GraphQlResponse, Operation, http::CynicReqwestError};
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use reqwest::Proxy;
use url::Url;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub struct Proxy;

/// API client for the Wasmer API.
///
/// Use the queries in [`crate::query`] to interact with the API.
#[derive(Clone, Debug)]
pub struct WasmerClient {
    auth_token: Option<String>,
    graphql_endpoint: Url,

    pub(crate) client: reqwest::Client,
    pub(crate) user_agent: reqwest::header::HeaderValue,
    #[allow(unused)]
    log_variables: bool,
}

impl WasmerClient {
    /// Env var used to enable logging of request variables.
    ///
    /// This is somewhat dangerous since it can log sensitive information, hence
    /// it is gated by a custom env var.
    const ENV_VAR_LOG_VARIABLES: &'static str = "WASMER_API_INSECURE_LOG_VARIABLES";

    pub fn graphql_endpoint(&self) -> &Url {
        &self.graphql_endpoint
    }

    pub fn auth_token(&self) -> Option<&str> {
        self.auth_token.as_deref()
    }

    fn parse_user_agent(user_agent: &str) -> Result<reqwest::header::HeaderValue, anyhow::Error> {
        if user_agent.is_empty() {
            bail!("user agent must not be empty");
        }
        user_agent
            .parse()
            .with_context(|| format!("invalid user agent: '{user_agent}'"))
    }

    pub fn new_with_client(
        client: reqwest::Client,
        graphql_endpoint: Url,
        user_agent: &str,
    ) -> Result<Self, anyhow::Error> {
        let log_variables = {
            let v = std::env::var(Self::ENV_VAR_LOG_VARIABLES).unwrap_or_default();
            match v.as_str() {
                "1" | "true" => true,
                "0" | "false" => false,
                // Default case if not provided.
                "" => false,
                other => {
                    bail!(
                        "invalid value for {} - expected 0/false|1/true: '{other}'",
                        Self::ENV_VAR_LOG_VARIABLES
                    );
                }
            }
        };

        Ok(Self {
            client,
            auth_token: None,
            user_agent: Self::parse_user_agent(user_agent)?,
            graphql_endpoint,
            log_variables,
        })
    }

    pub fn new(graphql_endpoint: Url, user_agent: &str) -> Result<Self, anyhow::Error> {
        Self::new_with_proxy(graphql_endpoint, user_agent, None)
    }

    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), allow(unused))]
    pub fn new_with_proxy(
        graphql_endpoint: Url,
        user_agent: &str,
        proxy: Option<Proxy>,
    ) -> Result<Self, anyhow::Error> {
        let builder = {
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            let builder = reqwest::ClientBuilder::new();

            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            let builder = reqwest::ClientBuilder::new()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(90));

            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            if let Some(proxy) = proxy {
                builder.proxy(proxy)
            } else {
                builder
            }
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            builder
        };

        let client = builder.build().context("failed to create reqwest client")?;

        Self::new_with_client(client, graphql_endpoint, user_agent)
    }

    pub fn with_auth_token(mut self, auth_token: String) -> Self {
        self.auth_token = Some(auth_token);
        self
    }

    pub(crate) async fn run_graphql_raw<ResponseData, Vars>(
        &self,
        operation: Operation<ResponseData, Vars>,
    ) -> Result<cynic::GraphQlResponse<ResponseData>, anyhow::Error>
    where
        Vars: serde::Serialize + std::fmt::Debug,
        ResponseData: serde::de::DeserializeOwned + std::fmt::Debug + 'static,
    {
        let req = self
            .client
            .post(self.graphql_endpoint.as_str())
            .header(reqwest::header::USER_AGENT, &self.user_agent);
        let req = if let Some(token) = &self.auth_token {
            req.bearer_auth(token)
        } else {
            req
        };

        let query = operation.query.clone();

        if self.log_variables {
            tracing::trace!(
                endpoint=%self.graphql_endpoint,
                query=serde_json::to_string(&operation).unwrap_or_default(),
                vars=?operation.variables,
                "sending graphql query"
            );
        } else {
            tracing::trace!(
                endpoint=%self.graphql_endpoint,
                query=serde_json::to_string(&operation).unwrap_or_default(),
                "sending graphql query"
            );
        }

        let res = req.json(&operation).send().await;

        let res = match res {
            Ok(response) => {
                let status = response.status();
                if !status.is_success() {
                    let body_string = match response.text().await {
                        Ok(b) => b,
                        Err(err) => {
                            tracing::error!("could not load response body: {err}");
                            "<could not retrieve body>".to_string()
                        }
                    };

                    match serde_json::from_str::<GraphQlResponse<ResponseData>>(&body_string) {
                        Ok(response) => Ok(response),
                        Err(_) => Err(CynicReqwestError::ErrorResponse(status, body_string)),
                    }
                } else {
                    let body = response.bytes().await?;

                    let jd = &mut serde_json::Deserializer::from_slice(&body);
                    let data: Result<GraphQlResponse<ResponseData>, _> =
                        serde_path_to_error::deserialize(jd).map_err(|err| {
                            let body_txt = String::from_utf8_lossy(&body);
                            CynicReqwestError::ErrorResponse(
                                reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Could not decode JSON response: {err} -- '{body_txt}'"),
                            )
                        });

                    data
                }
            }
            Err(e) => Err(CynicReqwestError::ReqwestError(e)),
        };
        let res = match res {
            Ok(res) => {
                tracing::trace!(?res, "GraphQL query succeeded");
                res
            }
            Err(err) => {
                tracing::error!(?err, "GraphQL query failed");
                return Err(err.into());
            }
        };

        if let Some(errors) = &res.errors {
            if !errors.is_empty() {
                tracing::warn!(
                    ?errors,
                    data=?res.data,
                    %query,
                    endpoint=%self.graphql_endpoint,
                    "GraphQL query succeeded, but returned errors",
                );
            }
        }

        Ok(res)
    }

    pub(crate) async fn run_graphql<ResponseData, Vars>(
        &self,
        operation: Operation<ResponseData, Vars>,
    ) -> Result<ResponseData, anyhow::Error>
    where
        Vars: serde::Serialize + std::fmt::Debug,
        ResponseData: serde::de::DeserializeOwned + std::fmt::Debug + 'static,
    {
        let res = self.run_graphql_raw(operation).await?;

        if let Some(data) = res.data {
            Ok(data)
        } else if let Some(errs) = res.errors {
            let errs = GraphQLApiFailure { errors: errs };
            Err(errs).context("GraphQL query failed")
        } else {
            Err(anyhow::anyhow!("Query did not return any data"))
        }
    }

    /// Run a GraphQL query, but fail (return an Error) if any error is returned
    /// in the response.
    pub(crate) async fn run_graphql_strict<ResponseData, Vars>(
        &self,
        operation: Operation<ResponseData, Vars>,
    ) -> Result<ResponseData, anyhow::Error>
    where
        Vars: serde::Serialize + std::fmt::Debug,
        ResponseData: serde::de::DeserializeOwned + std::fmt::Debug + 'static,
    {
        let res = self.run_graphql_raw(operation).await?;

        if let Some(errs) = res.errors {
            if !errs.is_empty() {
                let errs = GraphQLApiFailure { errors: errs };
                return Err(errs).context("GraphQL query failed");
            }
        }

        if let Some(data) = res.data {
            Ok(data)
        } else {
            Err(anyhow::anyhow!("Query did not return any data"))
        }
    }
}
