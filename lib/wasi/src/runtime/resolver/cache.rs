use std::{collections::HashMap, sync::RwLock};

use semver::VersionReq;

use crate::{
    bin_factory::BinaryPackage,
    http::HttpClient,
    runtime::resolver::{PackageResolver, ResolverError, WebcIdentifier},
};

/// A resolver that wraps a [`PackageResolver`] with an in-memory cache.
#[derive(Debug)]
pub struct InMemoryCache<R> {
    resolver: R,
    packages: RwLock<HashMap<String, Vec<BinaryPackage>>>,
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

    fn lookup(&self, package_name: &str, version_constraint: &VersionReq) -> Option<BinaryPackage> {
        let packages = self.packages.read().unwrap();
        let candidates = packages.get(package_name)?;

        let pkg = candidates
            .iter()
            .find(|pkg| version_constraint.matches(&pkg.version))?;

        Some(pkg.clone())
    }

    fn save(&self, pkg: BinaryPackage) {
        let mut packages = self.packages.write().unwrap();
        let candidates = packages.entry(pkg.package_name.clone()).or_default();
        candidates.push(pkg);
        // Note: We want to sort in descending order so lookups will always
        // yield the most recent compatible version.
        candidates.sort_by(|left, right| right.version.cmp(&left.version));
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
        if let Some(cached) = self.lookup(&ident.full_name, &ident.version) {
            // Cache hit!
            tracing::debug!(package=?ident, "The resolved package was already cached");
            return Ok(cached);
        }

        // the slow path
        let pkg = self.resolver.resolve_package(ident, client).await?;

        tracing::debug!(
            request.name = ident.full_name.as_str(),
            request.version = %ident.version,
            resolved.name = pkg.package_name.as_str(),
            resolved.version = %pkg.version,
            "Adding resolved package to the cache",
        );
        self.save(pkg.clone());

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

    fn dummy_pkg(name: &str, version: &str) -> BinaryPackage {
        BinaryPackage {
            package_name: name.into(),
            version: version.parse().unwrap(),
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
        cache.save(dummy_pkg("python/python", "0.0.0"));

        let pkg = cache
            .resolve_package(&ident, &DummyHttpClient)
            .await
            .unwrap();

        assert_eq!(pkg.version.to_string(), "0.0.0");
    }

    #[tokio::test]
    async fn semver_allows_wiggle_room_with_version_numbers() {
        let resolver = DummyResolver::default();
        let cache = InMemoryCache::new(resolver);
        cache.save(dummy_pkg("python/python", "1.0.0"));
        cache.save(dummy_pkg("python/python", "1.1.0"));
        cache.save(dummy_pkg("python/python", "2.0.0"));

        let pkg = cache
            .resolve_package(&"python/python@^1.0.5".parse().unwrap(), &DummyHttpClient)
            .await
            .unwrap();
        assert_eq!(pkg.version.to_string(), "1.1.0");

        let pkg = cache
            .resolve_package(&"python/python@1".parse().unwrap(), &DummyHttpClient)
            .await
            .unwrap();
        assert_eq!(pkg.version.to_string(), "1.1.0");

        let result = cache
            .resolve_package(&"python/python@=2.0.1".parse().unwrap(), &DummyHttpClient)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn cache_miss() {
        let resolver = DummyResolver::default();
        let cache = InMemoryCache::new(resolver);
        let ident: WebcIdentifier = "python/python".parse().unwrap();

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
