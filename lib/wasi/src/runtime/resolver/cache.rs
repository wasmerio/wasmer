use std::{collections::HashMap, sync::RwLock};

use crate::{
    bin_factory::BinaryPackage,
    http::HttpClient,
    runtime::resolver::{PackageResolver, ResolverError, WebcIdentifier},
};

/// A simple in-memory caching layer.
#[derive(Debug)]
pub struct InMemoryCache<R> {
    resolver: R,
    packages: RwLock<HashMap<WebcIdentifier, BinaryPackage>>,
}

impl<R> InMemoryCache<R> {
    pub fn new(resolver: R) -> Self {
        InMemoryCache {
            resolver,
            packages: RwLock::new(HashMap::new()),
        }
    }

    pub fn get_ref(&self) -> &R {
        &self.resolver
    }

    pub fn get_mut(&mut self) -> &mut R {
        &mut self.resolver
    }

    pub fn into_inner(self) -> R {
        self.resolver
    }
}

#[async_trait::async_trait]
impl<R> PackageResolver for InMemoryCache<R>
where
    R: PackageResolver,
{
    async fn resolve_package(
        &self,
        ident: WebcIdentifier,
        client: &(dyn HttpClient + Send + Sync),
    ) -> Result<BinaryPackage, ResolverError> {
        if let Some(cached) = self.packages.read().unwrap().get(&ident).cloned() {
            // Cache hit!
            tracing::debug!(package=?ident, "The resolved package was already cached");
            return Ok(cached);
        }

        // the slow path
        let pkg = self.resolver.resolve_package(ident.clone(), client).await?;

        tracing::debug!(
            request.name = ident.full_name.as_str(),
            request.version = ident.version.as_str(),
            resolved.name = pkg.package_name.as_str(),
            resolved.version = pkg.version.as_str(),
            "Adding resolved package to the cache",
        );
        self.packages.write().unwrap().insert(ident, pkg.clone());

        Ok(pkg)
    }
}
