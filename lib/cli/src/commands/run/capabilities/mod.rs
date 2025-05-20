use crate::config::WasmerEnv;

use super::PackageSource;
use anyhow::anyhow;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use wasmer_config::package::PackageSource as PackageSpecifier;

/// A custom implementation of the [`virtual_net::VirtualNetwork`] that asks users if they want to
/// use networking features at runtime.
pub(crate) mod net;

/// The default name of the directory to store cached capabilities for packages.
const DEFAULT_WASMER_PKG_CAPABILITY_CACHE_DIR: &str = "pkg_capabilities";

/// A struct representing cached capabilities for a specific package.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct PkgCapabilityCache {
    pub enable_networking: bool,
}

pub(crate) fn get_capability_cache_path(
    env: &WasmerEnv,
    input: &PackageSource,
) -> anyhow::Result<PathBuf> {
    let registry_name = env
        .registry_public_url()?
        .host_str()
        .unwrap_or("unknown_registry")
        .replace('.', "_");

    // We don't have the bytes of the module yet, but we still want to have the
    // package-capabilities cache be as close to an actual identifier as possible.
    let package_cache_path = match &input {
        PackageSource::File(f) => {
            let full_path = f.canonicalize()?.to_path_buf();
            let metadata = full_path
                .parent()
                .ok_or(anyhow!("No parent!"))?
                .metadata()?
                .modified()?;

            let mut hash = Sha256::new();
            hash.update(
                full_path
                    .into_os_string()
                    .into_string()
                    .map_err(|e| anyhow!("{e:?}"))?
                    .as_bytes(),
            );
            hash.update(
                metadata
                    .duration_since(UNIX_EPOCH)?
                    .as_millis()
                    .to_be_bytes(),
            );

            format!("path_{}.json", hex::encode(hash.finalize()))
        }
        PackageSource::Dir(f) => {
            let full_path = f.canonicalize()?.to_path_buf();
            let metadata = full_path.metadata()?.modified()?;

            let mut hash = Sha256::new();
            hash.update(
                full_path
                    .into_os_string()
                    .into_string()
                    .map_err(|e| anyhow!("{e:?}"))?
                    .as_bytes(),
            );
            hash.update(
                metadata
                    .duration_since(UNIX_EPOCH)?
                    .as_millis()
                    .to_be_bytes(),
            );

            format!("path_{}.json", hex::encode(hash.finalize()))
        }
        PackageSource::Package(p) => match p {
            PackageSpecifier::Ident(id) => match id {
                wasmer_config::package::PackageIdent::Named(n) => format!(
                    "ident_{}_{}",
                    n.namespace.clone().unwrap_or("unknown_namespace".into()),
                    n.name
                ),
                wasmer_config::package::PackageIdent::Hash(h) => {
                    format!("hash_{h}")
                }
            },
            PackageSpecifier::Path(f) => {
                let full_path = PathBuf::from(f).canonicalize()?.to_path_buf();

                let mut hasher = Sha256::new();
                hasher.update(
                    full_path
                        .clone()
                        .into_os_string()
                        .into_string()
                        .map_err(|e| anyhow!("{e:?}"))?
                        .as_bytes(),
                );

                if full_path.is_dir() {
                    hasher.update(
                        full_path
                            .metadata()?
                            .modified()?
                            .duration_since(UNIX_EPOCH)?
                            .as_millis()
                            .to_be_bytes(),
                    );
                } else if full_path.is_file() {
                    hasher.update(
                        full_path
                            .parent()
                            .ok_or(anyhow!("No parent!"))?
                            .metadata()?
                            .modified()?
                            .duration_since(UNIX_EPOCH)?
                            .as_millis()
                            .to_be_bytes(),
                    );
                }
                format!("path_{}.json", hex::encode(hasher.finalize()))
            }
            PackageSpecifier::Url(u) => {
                let mut hasher = Sha256::new();
                hasher.update(u.to_string().as_bytes());
                format!("path_{}.json", hex::encode(hasher.finalize()))
            }
        },
    };
    Ok(env
        .cache_dir()
        .join(DEFAULT_WASMER_PKG_CAPABILITY_CACHE_DIR)
        .join(registry_name)
        .join(package_cache_path))
}

pub(crate) fn get_cached_capability(path: &Path) -> anyhow::Result<PkgCapabilityCache> {
    let raw = std::fs::read_to_string(path)?;
    tracing::debug!("cache hit for package capability at {}", path.display());
    serde_json::from_str::<PkgCapabilityCache>(&raw)
        .map_err(|e| anyhow!("while deserializing package capability cache: {e:?}"))
}
