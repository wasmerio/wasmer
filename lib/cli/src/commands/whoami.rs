use clap::Parser;

use crate::opts::ApiOpts;

use super::AsyncCliCommand;

/// Show the current user.
///
/// Use this to verify you are currently logged in.
#[derive(Debug, Parser)]
pub struct Whoami {
    #[clap(flatten)]
    #[allow(missing_docs)]
    pub api: ApiOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for Whoami {
    type Output = wasmer_api::types::User;

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let client = self.api.client_authenticated()?;
        let user = wasmer_api::query::current_user(&client).await?;

        println!(
            "You are logged in as user '{}' @{}.",
            user.username,
            client.graphql_endpoint().host_str().unwrap_or_default(),
        );
        Ok(user)
    }
}
