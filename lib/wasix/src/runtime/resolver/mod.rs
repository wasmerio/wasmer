mod filesystem_source;
mod in_memory_source;
mod inputs;
mod multi_source_registry;
mod outputs;
mod resolve;
mod source;
pub(crate) mod utils;
mod wapm_source;
mod web_source;

pub use self::{
    filesystem_source::FileSystemSource,
    in_memory_source::InMemorySource,
    inputs::{
        Command, Dependency, DistributionInfo, PackageInfo, PackageSpecifier, PackageSummary,
        WebcHash,
    },
    multi_source_registry::MultiSource,
    outputs::{
        DependencyGraph, ItemLocation, PackageId, Resolution, ResolvedFileSystemMapping,
        ResolvedPackage,
    },
    resolve::resolve,
    source::Source,
    wapm_source::WapmSource,
    web_source::WebSource,
};
