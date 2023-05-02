use crate::{
    bin_factory::BinaryPackage,
    http::HttpClient,
    runtime::resolver::{PackageResolver, ResolverError, WebcIdentifier},
};

/// A [`PackageResolver`] that will try to resolve packages using a "primary"
/// resolver, falling back to another resolver if the first one fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackResolver<R1, R2> {
    primary: R1,
    fallback: R2,
}

impl<R1, R2> FallbackResolver<R1, R2> {
    pub fn new(primary: R1, fallback: R2) -> Self {
        FallbackResolver { primary, fallback }
    }

    pub fn primary(&self) -> &R1 {
        &self.primary
    }

    pub fn primary_mut(&mut self) -> &mut R1 {
        &mut self.primary
    }

    pub fn fallback(&self) -> &R2 {
        &self.fallback
    }

    pub fn fallback_mut(&mut self) -> &mut R2 {
        &mut self.fallback
    }

    pub fn into_inner(self) -> (R1, R2) {
        let FallbackResolver { primary, fallback } = self;
        (primary, fallback)
    }
}

#[async_trait::async_trait]
impl<R1, R2> PackageResolver for FallbackResolver<R1, R2>
where
    R1: PackageResolver,
    R2: PackageResolver,
{
    async fn resolve_package(
        &self,
        ident: &WebcIdentifier,
        client: &(dyn HttpClient + Send + Sync),
    ) -> Result<BinaryPackage, ResolverError> {
        let error = match self.primary.resolve_package(ident, client).await {
            Ok(pkg) => return Ok(pkg),
            Err(e) => e,
        };

        if let Ok(pkg) = self.fallback.resolve_package(ident, client).await {
            return Ok(pkg);
        }

        Err(error)
    }
}
