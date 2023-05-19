mod in_memory_source;
mod inputs;
mod multi_source_registry;
mod outputs;
mod registry;
mod resolve;
mod source;
mod wapm_source;
mod web_source;
mod filesystem_source;

pub use self::{
    in_memory_source::InMemorySource,
    filesystem_source::FileSystemSource,
    inputs::{
        Command, Dependency, DistributionInfo, PackageInfo, PackageSpecifier, PackageSummary,
        WebcHash,
    },
    multi_source_registry::MultiSourceRegistry,
    outputs::{
        DependencyGraph, FileSystemMapping, ItemLocation, PackageId, Resolution, ResolvedPackage,
    },
    registry::Registry,
    resolve::resolve,
    source::Source,
    wapm_source::WapmSource,
    web_source::WebSource,
};
