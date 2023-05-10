use anyhow::Error;
use url::Url;

use crate::runtime::resolver::{PackageSpecifier, Source, SourceId, SourceKind, Summary};

/// A [`Source`] which will resolve dependencies by pinging a WAPM-like GraphQL
/// endpoint.
#[derive(Debug, Clone)]
pub struct WapmSource {
    registry_endpoint: Url,
}

impl WapmSource {
    pub const WAPM_DEV_ENDPOINT: &str = "https://registry.wapm.dev/graphql";
    pub const WAPM_PROD_ENDPOINT: &str = "https://registry.wapm.io/graphql";
}

#[async_trait::async_trait]
impl Source for WapmSource {
    fn id(&self) -> SourceId {
        SourceId::new(SourceKind::Registry, self.registry_endpoint.clone())
    }

    async fn query(&self, _package: &PackageSpecifier) -> Result<Vec<Summary>, Error> {
        todo!();
    }
}
