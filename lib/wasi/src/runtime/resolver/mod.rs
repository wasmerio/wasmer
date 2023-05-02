mod registry;
mod cache;
mod fallback;
mod preloaded;
mod types;

pub use self::{
    registry::RegistryResolver,
    cache::InMemoryCache,
    fallback::FallbackResolver,
    preloaded::PreloadedResolver,
    types::{
        FileSystemMapping, Locator, PackageResolver, ResolvedCommand, ResolvedPackage,
        ResolverError, WebcIdentifier,
    },
};
