use std::{collections::HashMap, sync::RwLock};

use crate::{
    bin_factory::BinaryPackage,
    http::HttpClient,
    runtime::resolver::{PackageResolver, ResolverError, WebcIdentifier},
};

/// A resolver that wraps a [`PackageResolver`] in an in-memory LRU cache.
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

    fn lookup(&self, ident: &WebcIdentifier) -> Option<BinaryPackage> {
        self.packages.read().unwrap().get(ident).cloned()
    }

    fn save(&self, ident: &WebcIdentifier, pkg: BinaryPackage) {
        let mut packages = self.packages.write().unwrap();
        packages.insert(ident.clone(), pkg);
    }
}

#[async_trait::async_trait]
impl<R> PackageResolver for InMemoryCache<R>
where
    R: PackageResolver,
{
    async fn resolve_package(
        &self,
        ident: &WebcIdentifier,
        client: &(dyn HttpClient + Send + Sync),
    ) -> Result<BinaryPackage, ResolverError> {
        if let Some(cached) = self.lookup(ident) {
            // Cache hit!
            tracing::debug!(package=?ident, "The resolved package was already cached");
            return Ok(cached);
        }

        // the slow path
        let pkg = self.resolver.resolve_package(ident, client).await?;

        tracing::debug!(
            request.name = ident.full_name.as_str(),
            request.version = ident.version.as_str(),
            resolved.name = pkg.package_name.as_str(),
            resolved.version = pkg.version.as_str(),
            "Adding resolved package to the cache",
        );
        self.save(ident, pkg.clone());

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Debug, Default)]
    struct DummyResolver {
        calls: Mutex<Vec<WebcIdentifier>>,
    }

    #[async_trait::async_trait]
    impl PackageResolver for DummyResolver {
        async fn resolve_package(
            &self,
            ident: &WebcIdentifier,
            _client: &(dyn HttpClient + Send + Sync),
        ) -> Result<BinaryPackage, ResolverError> {
            self.calls.lock().unwrap().push(ident.clone());
            Err(ResolverError::UnknownPackage(ident.clone()))
        }
    }

    fn dummy_pkg(name: impl Into<String>) -> BinaryPackage {
        BinaryPackage {
            package_name: name.into(),
            version: "0.0.0".to_string(),
            when_cached: None,
            entry: None,
            hash: Arc::default(),
            webc_fs: None,
            commands: Arc::default(),
            uses: Vec::new(),
            module_memory_footprint: 0,
            file_system_memory_footprint: 0,
        }
    }

    #[derive(Debug)]
    struct DummyHttpClient;

    impl HttpClient for DummyHttpClient {
        fn request(
            &self,
            _request: crate::http::HttpRequest,
        ) -> futures::future::BoxFuture<'_, Result<crate::http::HttpResponse, anyhow::Error>>
        {
            todo!()
        }
    }

    #[tokio::test]
    async fn cache_hit() {
        let resolver = DummyResolver::default();
        let cache = InMemoryCache::new(resolver);
        let ident: WebcIdentifier = "python/python".parse().unwrap();
        cache.save(&ident, dummy_pkg("python/python"));

        let pkg = cache
            .resolve_package(&ident, &DummyHttpClient)
            .await
            .unwrap();

        assert_eq!(pkg.version, "0.0.0");
    }

    #[tokio::test]
    async fn cache_miss() {
        let resolver = DummyResolver::default();
        let cache = InMemoryCache::new(resolver);
        let ident: WebcIdentifier = "python/python".parse().unwrap();
        assert!(cache.lookup(&ident).is_none());

        let expected_err = cache
            .resolve_package(&ident, &DummyHttpClient)
            .await
            .unwrap_err();

        assert!(matches!(expected_err, ResolverError::UnknownPackage(_)));
        // there should have been one call to the wrapped resolver
        let calls = cache.get_ref().calls.lock().unwrap();
        assert_eq!(&*calls, &[ident]);
    }
}
