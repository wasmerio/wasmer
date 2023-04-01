use wasmer_types::SerializableModule;

/// Lock that prevents multiple compilers from compiling the same
/// module in the background (this is a blocking guard)
pub trait TieredCachingGuard {}

/// Abstracts the caching sub-system used by the tiered compiler
pub trait TieredCaching
where
    Self: Send + Sync + std::fmt::Debug,
{
    fn try_load(&self, hash: u128) -> Option<SerializableModule>;

    fn lock(&self, hash: u128) -> Box<dyn TieredCachingGuard>;

    fn store(&self, hash: u128, compilation: &SerializableModule);
}

#[derive(Debug, Default)]
pub struct DefaultTieredCaching {}

#[derive(Default)]
struct DefaultTieredCachingGuard {}
impl TieredCachingGuard for DefaultTieredCachingGuard {}

impl TieredCaching for DefaultTieredCaching {
    fn try_load(&self, _hash: u128) -> Option<SerializableModule> {
        None
    }

    fn store(&self, _hash: u128, _compilation: &SerializableModule) {}

    fn lock(&self, _hash: u128) -> Box<dyn TieredCachingGuard> {
        Box::new(DefaultTieredCachingGuard::default())
    }
}
