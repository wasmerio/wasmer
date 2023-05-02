use crate::{
    bin_factory::BinaryPackage,
    http::HttpClient,
    runtime::resolver::{PackageResolver, ResolverError, WebcIdentifier},
};

/// A [`PackageResolver`] that only knows about a handful of hard-coded packages.
#[derive(Debug, Clone)]
pub struct PreloadedResolver {
    packages: Vec<BinaryPackage>,
}

impl PreloadedResolver {
    pub fn new(packages: Vec<BinaryPackage>) -> Self {
        PreloadedResolver { packages }
    }
}

#[async_trait::async_trait]
impl PackageResolver for PreloadedResolver {
    async fn resolve_package(
        &self,
        ident: &WebcIdentifier,
        _client: &(dyn HttpClient + Send + Sync),
    ) -> Result<BinaryPackage, ResolverError> {
        for pkg in &self.packages {
            // TODO: We need to make the WebcIdentifier semver-aware. That way
            // the resolve_package() can pass in a constraint and we'll be able
            // to match it against a concrete version.
            if pkg.package_name == ident.full_name && pkg.version == ident.version {
                return Ok(pkg.clone());
            }
        }

        Err(ResolverError::UnknownPackage(ident.clone()))
    }
}
