use crate::runtime::resolver::{PackageResolver, ResolvedPackage, ResolverError, WebcIdentifier};

use webc::metadata::Manifest;

/// The default package resolver, backed by WAPM.
#[derive(Debug, Default, Clone)]
pub struct DefaultResolver {}

#[async_trait::async_trait]
impl PackageResolver for DefaultResolver {
    async fn load_manifest(&self, _pkg: WebcIdentifier) -> Result<Manifest, ResolverError> {
        todo!();
    }

    async fn resolve_package(
        &self,
        _pkg: WebcIdentifier,
    ) -> Result<ResolvedPackage, ResolverError> {
        todo!();
    }
}
