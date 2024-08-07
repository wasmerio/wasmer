mod backend_source;
mod filesystem_source;
mod in_memory_source;
mod inputs;
mod multi_source;
mod outputs;
mod resolve;
mod source;
pub(crate) mod utils;
mod web_source;

pub use self::{
    backend_source::BackendSource,
    filesystem_source::FileSystemSource,
    in_memory_source::InMemorySource,
    inputs::{
        Command, Dependency, DistributionInfo, FileSystemMapping, PackageInfo, PackageSummary,
        WebcHash,
    },
    multi_source::{MultiSource, MultiSourceStrategy},
    outputs::{
        DependencyGraph, Edge, ItemLocation, Node, Resolution, ResolvedFileSystemMapping,
        ResolvedPackage,
    },
    resolve::{resolve, ResolveError},
    source::{QueryError, Source},
    web_source::WebSource,
};
