use wasmer::{Engine, Module};

use crate::runtime::module_cache::{CacheError, ModuleCache, ModuleHash};

/// [`FallbackCache`] is a combinator for the [`ModuleCache`] trait that enables
/// the chaining of two caching strategies together, typically via
/// [`ModuleCache::with_fallback()`].
///
/// All operations are attempted using primary cache first, and if that fails,
/// falls back to using the fallback cache. By chaining different caches
/// together with [`FallbackCache`], you can create a caching hierarchy tailored
/// to your application's specific needs, balancing performance, resource usage,
/// and persistence.
///
/// A key assumption of [`FallbackCache`] is that **all operations on the
/// fallback implementation will be significantly slower than the primary one**.
///
/// ## Cache Promotion
///
/// Whenever there is a cache miss on the primary cache and the fallback is
/// able to load a module, that module is automatically added to the primary
/// cache to improve the speed of future lookups.
///
/// This "cache promotion" strategy helps keep frequently accessed modules in
/// the faster primary cache.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FallbackCache<Primary, Fallback> {
    primary: Primary,
    fallback: Fallback,
}

impl<Primary, Fallback> FallbackCache<Primary, Fallback> {
    pub(crate) fn new(primary: Primary, fallback: Fallback) -> Self {
        FallbackCache { primary, fallback }
    }

    pub fn primary(&self) -> &Primary {
        &self.primary
    }

    pub fn primary_mut(&mut self) -> &mut Primary {
        &mut self.primary
    }

    pub fn fallback(&self) -> &Fallback {
        &self.fallback
    }

    pub fn fallback_mut(&mut self) -> &mut Fallback {
        &mut self.fallback
    }

    pub fn into_inner(self) -> (Primary, Fallback) {
        let FallbackCache { primary, fallback } = self;
        (primary, fallback)
    }
}

#[async_trait::async_trait]
impl<Primary, Fallback> ModuleCache for FallbackCache<Primary, Fallback>
where
    Primary: ModuleCache + Send + Sync,
    Fallback: ModuleCache + Send + Sync,
{
    async fn load(&self, key: ModuleHash, engine: &Engine) -> Result<Module, CacheError> {
        let primary_error = match self.primary.load(key, engine).await {
            Ok(m) => return Ok(m),
            Err(e) => e,
        };

        if let Ok(m) = self.fallback.load(key, engine).await {
            // Now we've got a module, let's make sure it is promoted to the
            // primary cache.
            if let Err(e) = self.primary.save(key, engine, &m).await {
                tracing::warn!(
                    %key,
                    error = &e as &dyn std::error::Error,
                    "Unable to promote a module to the primary cache",
                );
            }

            return Ok(m);
        }

        Err(primary_error)
    }

    async fn save(
        &self,
        key: ModuleHash,
        engine: &Engine,
        module: &Module,
    ) -> Result<(), CacheError> {
        futures::try_join!(
            self.primary.save(key, engine, module),
            self.fallback.save(key, engine, module)
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::runtime::module_cache::SharedCache;

    const ADD_WAT: &[u8] = br#"(
        module
            (func
                (export "add")
                (param $x i64)
                (param $y i64)
                (result i64)
                (i64.add (local.get $x) (local.get $y)))
        )"#;

    #[derive(Debug)]
    struct Spy<I> {
        inner: I,
        success: AtomicUsize,
        failures: AtomicUsize,
    }

    impl<I> Spy<I> {
        fn new(inner: I) -> Self {
            Spy {
                inner,
                success: AtomicUsize::new(0),
                failures: AtomicUsize::new(0),
            }
        }

        fn success(&self) -> usize {
            self.success.load(Ordering::SeqCst)
        }

        fn failures(&self) -> usize {
            self.failures.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl<I: ModuleCache + Send + Sync> ModuleCache for Spy<I> {
        async fn load(&self, key: ModuleHash, engine: &Engine) -> Result<Module, CacheError> {
            match self.inner.load(key, engine).await {
                Ok(m) => {
                    self.success.fetch_add(1, Ordering::SeqCst);
                    Ok(m)
                }
                Err(e) => {
                    self.failures.fetch_add(1, Ordering::SeqCst);
                    Err(e)
                }
            }
        }

        async fn save(
            &self,
            key: ModuleHash,
            engine: &Engine,
            module: &Module,
        ) -> Result<(), CacheError> {
            match self.inner.save(key, engine, module).await {
                Ok(_) => {
                    self.success.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
                Err(e) => {
                    self.failures.fetch_add(1, Ordering::SeqCst);
                    Err(e)
                }
            }
        }
    }

    #[tokio::test]
    async fn load_from_primary() {
        let engine = Engine::default();
        let module = Module::new(&engine, ADD_WAT).unwrap();
        let key = ModuleHash::xxhash_from_bytes([0; 8]);
        let primary = SharedCache::default();
        let fallback = SharedCache::default();
        primary.save(key, &engine, &module).await.unwrap();
        let primary = Spy::new(primary);
        let fallback = Spy::new(fallback);
        let cache = FallbackCache::new(&primary, &fallback);

        let got = cache.load(key, &engine).await.unwrap();

        // We should have received the same module
        assert_eq!(module, got);
        assert_eq!(primary.success(), 1);
        assert_eq!(primary.failures(), 0);
        // but the fallback wasn't touched at all
        assert_eq!(fallback.success(), 0);
        assert_eq!(fallback.failures(), 0);
        // And the fallback still doesn't have our module
        assert!(fallback.load(key, &engine).await.is_err());
    }

    #[tokio::test]
    async fn loading_from_fallback_also_populates_primary() {
        let engine = Engine::default();
        let module = Module::new(&engine, ADD_WAT).unwrap();
        let key = ModuleHash::xxhash_from_bytes([0; 8]);
        let primary = SharedCache::default();
        let fallback = SharedCache::default();
        fallback.save(key, &engine, &module).await.unwrap();
        let primary = Spy::new(primary);
        let fallback = Spy::new(fallback);
        let cache = FallbackCache::new(&primary, &fallback);

        let got = cache.load(key, &engine).await.unwrap();

        // We should have received the same module
        assert_eq!(module, got);
        // We got a hit on the fallback
        assert_eq!(fallback.success(), 1);
        assert_eq!(fallback.failures(), 0);
        // the load() on our primary failed
        assert_eq!(primary.failures(), 1);
        // but afterwards, we updated the primary cache with our module
        assert_eq!(primary.success(), 1);
        assert_eq!(primary.load(key, &engine).await.unwrap(), module);
    }

    #[tokio::test]
    async fn saving_will_update_both() {
        let engine = Engine::default();
        let module = Module::new(&engine, ADD_WAT).unwrap();
        let key = ModuleHash::xxhash_from_bytes([0; 8]);
        let primary = SharedCache::default();
        let fallback = SharedCache::default();
        let cache = FallbackCache::new(&primary, &fallback);

        cache.save(key, &engine, &module).await.unwrap();

        assert_eq!(primary.load(key, &engine).await.unwrap(), module);
        assert_eq!(fallback.load(key, &engine).await.unwrap(), module);
    }
}
