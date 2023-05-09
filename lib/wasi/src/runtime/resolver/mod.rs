mod cache;
mod registry;
mod types;

pub use self::{
    cache::InMemoryCache,
    registry::RegistryResolver,
    types::{
        FileSystemMapping, Locator, PackageResolver, ResolvedCommand, ResolvedPackage,
        ResolverError, WebcIdentifier,
    },
};
