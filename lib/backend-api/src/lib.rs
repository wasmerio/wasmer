// Allowed because it makes code more readable.
#![allow(clippy::bool_comparison, clippy::match_like_matches_macro)]

mod client;
mod error;

pub mod global_id;
pub mod query;
pub mod stream;
#[cfg(feature = "sys")]
pub mod subscription;
pub mod types;

use url::Url;

pub use self::{client::WasmerClient, error::GraphQLApiFailure};

/// Api endpoint for the dev environment.
pub const ENDPOINT_DEV: &str = "https://registry.wasmer.wtf/graphql";
/// Api endpoint for the prod environment.
pub const ENDPOINT_PROD: &str = "https://registry.wasmer.io/graphql";

/// API endpoint for the dev environment.
pub fn endpoint_dev() -> Url {
    Url::parse(ENDPOINT_DEV).unwrap()
}

/// API endpoint for the prod environment.
pub fn endpoint_prod() -> Url {
    Url::parse(ENDPOINT_PROD).unwrap()
}
