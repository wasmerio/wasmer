mod registry;
mod cache;
mod types;

pub use self::{
    registry::RegistryResolver,
    cache::InMemoryCache,
    types::{
        FileSystemMapping, Locator, PackageResolver, ResolvedCommand, ResolvedPackage,
        ResolverError, WebcIdentifier,
    },
};
