mod in_memory_source;
mod inputs;
mod multi_source_registry;
mod outputs;
mod registry;
mod resolve;
mod source;
mod utils;
mod wapm_source;

pub use self::{
    in_memory_source::InMemorySource,
    inputs::{Command, Dependency, PackageSpecifier, Summary, WebcHash},
    multi_source_registry::MultiSourceRegistry,
    outputs::{
        DependencyGraph, FileSystemMapping, ItemLocation, PackageId, Resolution, ResolvedPackage,
    },
    registry::Registry,
    resolve::resolve,
    source::{Source, SourceId, SourceKind},
    wapm_source::WapmSource,
};

pub(crate) use self::utils::{extract_summary_from_manifest, extract_summary_from_webc};
