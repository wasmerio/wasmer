/// A custom implementation of the [`virtual_net::VirtualNetwork`] that asks users if they want to
/// use networking features at runtime.
pub(crate) mod net;

/// The default name of the directory to store cached capabilities for packages.
pub(crate) const DEFAULT_WASMER_PKG_CAPABILITY_CACHE_DIR: &str = "pkg_capabilities";

/// A struct representing cached capabilities for a specific package.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct PkgCapabilityCache {
    pub enable_networking: bool,
}
