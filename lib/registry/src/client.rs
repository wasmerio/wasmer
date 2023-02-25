use anyhow::Context;
use graphql_client::GraphQLQuery;
use url::Url;

use crate::{
    graphql,
};

/// API client for the Wasmer registry.
#[derive(Clone)]
pub struct RegistryClient {
    client: reqwest::Client,
    endpoint: Url,
    token: Option<String>,
}

impl RegistryClient {
    /// Construct a new registry.
    pub fn new(endpoint: Url, token: Option<String>, user_agent: Option<String>) -> Self {
        let user_agent = user_agent.unwrap_or_else(Self::default_user_agent);
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .build()
            .unwrap();
        Self {
            client,
            endpoint,
            token,
        }
    }

    /// Construct a client from a [`crate::config::RegistryLogin`].
    pub fn from_registry(registry: crate::config::RegistryLogin) -> Result<Self, anyhow::Error> {
        let endpoint = registry.registry.parse().context("Invalid registry URL")?;
        let client = Self::new(endpoint, Some(registry.token), None);
        Ok(client)
    }

    pub(crate) fn default_user_agent() -> String {
        format!(
            "wasmer/{} {} {}",
            env!("CARGO_PKG_VERSION"),
            whoami::platform(),
            crate::graphql::whoami_distro(),
        )
    }

    /// Set the GraphQL API endpoint.
    pub fn with_endpoint(self, endpoint: Url) -> Self {
        Self { endpoint, ..self }
    }

    /// Set the authentication token.
    pub fn with_token(self, token: String) -> Self {
        Self {
            token: Some(token),
            ..self
        }
    }

    /// Execute a GraphQL query.
    async fn execute<Q: GraphQLQuery>(
        &self,
        vars: Q::Variables,
    ) -> Result<graphql_client::Response<Q::ResponseData>, reqwest::Error> {
        let body = Q::build_query(vars);

        let req = self.client.post(self.endpoint.as_str());

        let req = if let Some(token) = &self.token {
            req.bearer_auth(token)
        } else {
            req
        };
        req.json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<graphql_client::Response<Q::ResponseData>>()
            .await
    }

    /// Execute a GraphQL query, and convert a response with errors to a Rust error.
    async fn execute_checked<Q: GraphQLQuery>(
        &self,
        vars: Q::Variables,
    ) -> Result<Q::ResponseData, anyhow::Error> {
        let res = self.execute::<Q>(vars).await?;

        if let Some(data) = res.data {
            Ok(data)
        } else {
            // TODO: Better error forwaring with a custom error type.
            anyhow::bail!("GraphQL error: {:?}", res.errors);
        }
    }
}
