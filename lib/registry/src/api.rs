use anyhow::Context;

use crate::RegistryClient;

use crate::graphql::mutations::{self};
use crate::types::NewNonceOutput;

/// Generate a new Nonce
///
/// Takes a name and a callbackUrl and returns a nonce
pub async fn create_nonce(
    client: &RegistryClient,
    name: String,
    callback_url: String,
) -> Result<NewNonceOutput, anyhow::Error> {
    let vars = mutations::new_nonce::Variables { name, callback_url };
    let nonce = client
        .execute::<mutations::NewNonce>(vars)
        .await?
        .new_nonce
        .context("Query did not return a nonce")?
        .nonce;

    Ok(NewNonceOutput {
        auth_url: nonce.auth_url,
    })
}
