mod filesystem_source;
mod in_memory_source;
mod inputs;
mod multi_source;
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
        Command, Dependency, DistributionInfo, FileSystemMapping, PackageInfo, PackageSpecifier,
        PackageSummary, WebcHash,
    },
    multi_source::{MultiSource, MultiSourceStrategy},
    outputs::{
        DependencyGraph, Edge, ItemLocation, Node, PackageId, Resolution,
        ResolvedFileSystemMapping, ResolvedPackage,
    },
    resolve::{resolve, ResolveError},
    source::{QueryError, Source},
    wapm_source::WapmSource,
    web_source::WebSource,
};
