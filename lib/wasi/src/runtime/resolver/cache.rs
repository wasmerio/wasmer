use std::{sync::Mutex, time::Instant};

use crate::{
    bin_factory::BinaryPackage,
    http::HttpClient,
    runtime::resolver::{PackageResolver, ResolverError, WebcIdentifier},
};

/// A resolver that wraps a [`PackageResolver`] in an in-memory LRU cache.
#[derive(Debug)]
pub struct InMemoryCache<R> {
    resolver: R,
    packages: Mutex<Vec<CacheEntry>>,
    config: CacheConfig,
}

impl<R> InMemoryCache<R> {
    pub fn new(resolver: R, config: CacheConfig) -> Self {
        InMemoryCache {
            resolver,
            packages: Mutex::new(Vec::new()),
            config,
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
        let mut packages = self.packages.lock().unwrap();

        let now = Instant::now();
        let entry = packages.iter_mut().find(|entry| entry.ident == *ident)?;
        entry.last_touched = now;
        let pkg = entry.pkg.clone();

        self.config.prune(&mut packages, now);

        Some(pkg)
    }

    fn save(&self, ident: &WebcIdentifier, pkg: BinaryPackage) {
        let mut packages = self.packages.lock().unwrap();
        packages.insert(
            0,
            CacheEntry {
                last_touched: Instant::now(),
                ident: ident.clone(),
                pkg,
            },
        );

        self.config.prune(&mut packages, Instant::now());
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

#[derive(Debug, Clone)]
struct CacheEntry {
    last_touched: Instant,
    ident: WebcIdentifier,
    pkg: BinaryPackage,
}

impl CacheEntry {
    fn approximate_memory_usage(&self) -> u64 {
        self.pkg.file_system_memory_footprint + self.pkg.module_memory_footprint
    }
}

/// Configuration for the [`InMemoryCache`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CacheConfig {
    /// The maximum amount of data that should be held in memory before dropping
    /// cached artefacts.
    pub max_memory_usage: Option<u64>,
}

impl CacheConfig {
    fn prune(&self, packages: &mut Vec<CacheEntry>, now: Instant) {
        // Note that we run this function while the cache lock is held, so we
        // should prefer faster cache invalidation strategies over more accurate
        // ones. It's also important to not block.

        packages.sort_by_key(|entry| now.duration_since(entry.last_touched));

        if let Some(limit) = self.max_memory_usage {
            let mut memory_used = 0;

            // Note: retain()'s closure is guaranteed to be run on each entry in
            // order.
            packages.retain(|entry| {
                memory_used += entry.approximate_memory_usage();
                memory_used < limit
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

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
        let cache = InMemoryCache::new(resolver, CacheConfig::default());
        let ident: WebcIdentifier = "python/python".parse().unwrap();
        cache.packages.lock().unwrap().push(CacheEntry {
            last_touched: Instant::now(),
            ident: ident.clone(),
            pkg: dummy_pkg("python/python"),
        });

        let pkg = cache
            .resolve_package(&ident, &DummyHttpClient)
            .await
            .unwrap();

        assert_eq!(pkg.version, "0.0.0");
    }

    #[tokio::test]
    async fn cache_miss() {
        let resolver = DummyResolver::default();
        let cache = InMemoryCache::new(resolver, CacheConfig::default());
        let ident: WebcIdentifier = "python/python".parse().unwrap();
        assert!(cache.packages.lock().unwrap().is_empty());

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
