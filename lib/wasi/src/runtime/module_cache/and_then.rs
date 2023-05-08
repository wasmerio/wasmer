use wasmer::{Engine, Module};

use crate::runtime::module_cache::{CacheError, ModuleCache, ModuleHash};

/// A [`ModuleCache`] combinator which will try operations on one cache
/// and fall back to a secondary cache if they fail.
///
/// Constructed via [`ModuleCache::and_then()`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AndThen<Primary, Secondary> {
    primary: Primary,
    secondary: Secondary,
}

impl<Primary, Secondary> AndThen<Primary, Secondary> {
    pub(crate) fn new(primary: Primary, secondary: Secondary) -> Self {
        AndThen { primary, secondary }
    }

    pub fn primary(&self) -> &Primary {
        &self.primary
    }

    pub fn primary_mut(&mut self) -> &mut Primary {
        &mut self.primary
    }

    pub fn secondary(&self) -> &Secondary {
        &self.secondary
    }

    pub fn secondary_mut(&mut self) -> &mut Secondary {
        &mut self.secondary
    }

    pub fn into_inner(self) -> (Primary, Secondary) {
        let AndThen { primary, secondary } = self;
        (primary, secondary)
    }
}

#[async_trait::async_trait]
impl<Primary, Secondary> ModuleCache for AndThen<Primary, Secondary>
where
    Primary: ModuleCache + Send + Sync,
    Secondary: ModuleCache + Send + Sync,
{
    async fn load(&self, key: ModuleHash, engine: &Engine) -> Result<Module, CacheError> {
        let primary_error = match self.primary.load(key, engine).await {
            Ok(m) => return Ok(m),
            Err(e) => e,
        };

        if let Ok(m) = self.secondary.load(key, engine).await {
            // Now we've got a module, let's make sure it ends up in the primary
            // cache too.
            if let Err(e) = self.primary.save(key, engine, &m).await {
                tracing::warn!(
                    %key,
                    error = &e as &dyn std::error::Error,
                    "Unable to save a module to the primary cache",
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
            self.secondary.save(key, engine, module)
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
        let key = ModuleHash::from_raw([0; 32]);
        let primary = SharedCache::default();
        let secondary = SharedCache::default();
        primary.save(key, &engine, &module).await.unwrap();
        let primary = Spy::new(primary);
        let secondary = Spy::new(secondary);
        let cache = AndThen::new(&primary, &secondary);

        let got = cache.load(key, &engine).await.unwrap();

        // We should have received the same module
        assert_eq!(module, got);
        assert_eq!(primary.success(), 1);
        assert_eq!(primary.failures(), 0);
        // but the secondary wasn't touched at all
        assert_eq!(secondary.success(), 0);
        assert_eq!(secondary.failures(), 0);
        // And the secondary still doesn't have our module
        assert!(secondary.load(key, &engine).await.is_err());
    }

    #[tokio::test]
    async fn loading_from_secondary_also_populates_primary() {
        let engine = Engine::default();
        let module = Module::new(&engine, ADD_WAT).unwrap();
        let key = ModuleHash::from_raw([0; 32]);
        let primary = SharedCache::default();
        let secondary = SharedCache::default();
        secondary.save(key, &engine, &module).await.unwrap();
        let primary = Spy::new(primary);
        let secondary = Spy::new(secondary);
        let cache = AndThen::new(&primary, &secondary);

        let got = cache.load(key, &engine).await.unwrap();

        // We should have received the same module
        assert_eq!(module, got);
        // We got a hit on the secondary
        assert_eq!(secondary.success(), 1);
        assert_eq!(secondary.failures(), 0);
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
        let key = ModuleHash::from_raw([0; 32]);
        let primary = SharedCache::default();
        let secondary = SharedCache::default();
        let cache = AndThen::new(&primary, &secondary);

        cache.save(key, &engine, &module).await.unwrap();

        assert_eq!(primary.load(key, &engine).await.unwrap(), module);
        assert_eq!(secondary.load(key, &engine).await.unwrap(), module);
    }
}
