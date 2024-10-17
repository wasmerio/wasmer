mod dummy_waker;
mod owned_mutex_guard;
pub mod store;
mod thread_parker;

#[cfg(feature = "js")]
pub(crate) mod web;

pub use self::{dummy_waker::WasiDummyWaker, thread_parker::WasiParkingLot};

pub(crate) use owned_mutex_guard::{
    read_owned, write_owned, OwnedRwLockReadGuard, OwnedRwLockWriteGuard,
};
use rand::RngCore;
use wasmer_types::ModuleHash;

use std::collections::BTreeSet;

use wasmer::Module;
use wasmer_wasix_types::wasi::Errno;

/// Creates a random xxhash for the module
pub fn xxhash_random() -> ModuleHash {
    let mut rand = rand::thread_rng();
    let mut key = [0u8; 8];
    rand.fill_bytes(&mut key);
    ModuleHash::xxhash_from_bytes(key)
}

/// Check if a provided module is compiled for some version of WASI.
/// Use [`get_wasi_version`] to find out which version of WASI the module is.
pub fn is_wasi_module(module: &Module) -> bool {
    get_wasi_version(module, false).is_some()
}

/// Returns if the module is WASIX or not
pub fn is_wasix_module(module: &Module) -> bool {
    match get_wasi_versions(module, false).ok_or(false) {
        Ok(wasi_versions) => {
            wasi_versions.contains(&WasiVersion::Wasix32v1)
                || wasi_versions.contains(&WasiVersion::Wasix64v1)
        }
        Err(_) => false,
    }
}

pub fn map_io_err(err: std::io::Error) -> Errno {
    From::<std::io::Error>::from(err)
}

#[cfg(feature = "journal")]
pub fn map_snapshot_err(err: anyhow::Error) -> Errno {
    tracing::warn!("unknown snapshot error: {}", err);
    Errno::Unknown
}

/// The version of WASI. This is determined by the imports namespace
/// string.
#[derive(Debug, Clone, Copy, Eq)]
pub enum WasiVersion {
    /// `wasi_unstable`.
    Snapshot0,

    /// `wasi_snapshot_preview1`.
    Snapshot1,

    /// `wasix_32v1`.
    Wasix32v1,

    /// `wasix_64v1`.
    Wasix64v1,

    /// Latest version.
    ///
    /// It's a “floating” version, i.e. it's an alias to the latest
    /// version (for the moment, `Snapshot1`). Using this version is a
    /// way to ensure that modules will run only if they come with the
    /// latest WASI version (in case of security issues for instance),
    /// by just updating the runtime.
    ///
    /// Note that this version is never returned by an API. It is
    /// provided only by the user.
    Latest,
}

impl WasiVersion {
    /// Get the version as its namespace str as it appears in Wasm modules.
    pub const fn get_namespace_str(&self) -> &'static str {
        match *self {
            WasiVersion::Snapshot0 => SNAPSHOT0_NAMESPACE,
            WasiVersion::Snapshot1 => SNAPSHOT1_NAMESPACE,
            WasiVersion::Wasix32v1 => WASIX_32V1_NAMESPACE,
            WasiVersion::Wasix64v1 => WASIX_64V1_NAMESPACE,
            WasiVersion::Latest => SNAPSHOT1_NAMESPACE,
        }
    }
}

impl PartialEq<WasiVersion> for WasiVersion {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (*self, *other),
            (Self::Snapshot1, Self::Latest)
                | (Self::Latest, Self::Snapshot1)
                | (Self::Latest, Self::Latest)
                | (Self::Snapshot0, Self::Snapshot0)
                | (Self::Snapshot1, Self::Snapshot1)
                | (Self::Wasix32v1, Self::Wasix32v1)
                | (Self::Wasix64v1, Self::Wasix64v1)
        )
    }
}

impl PartialOrd for WasiVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for WasiVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self == other {
            return std::cmp::Ordering::Equal;
        }
        match (*self, *other) {
            (Self::Snapshot1, Self::Snapshot0) => std::cmp::Ordering::Greater,
            (Self::Wasix32v1, Self::Snapshot1) | (Self::Wasix32v1, Self::Snapshot0) => {
                std::cmp::Ordering::Greater
            }
            (Self::Wasix64v1, Self::Wasix32v1)
            | (Self::Wasix64v1, Self::Snapshot1)
            | (Self::Wasix64v1, Self::Snapshot0) => std::cmp::Ordering::Greater,
            (Self::Latest, Self::Wasix64v1)
            | (Self::Latest, Self::Wasix32v1)
            | (Self::Latest, Self::Snapshot1)
            | (Self::Latest, Self::Snapshot0) => std::cmp::Ordering::Greater,
            // they are not equal and not greater so they must be less
            (_, _) => std::cmp::Ordering::Less,
        }
    }
}

/// Namespace for the `Snapshot0` version.
const SNAPSHOT0_NAMESPACE: &str = "wasi_unstable";

/// Namespace for the `Snapshot1` version.
const SNAPSHOT1_NAMESPACE: &str = "wasi_snapshot_preview1";

/// Namespace for the `wasix` version.
const WASIX_32V1_NAMESPACE: &str = "wasix_32v1";

/// Namespace for the `wasix` version.
const WASIX_64V1_NAMESPACE: &str = "wasix_64v1";

/// Namespace for the `wasix` version.
const WASIX_HTTP_V1_NAMESPACE: &str = "wasix_http_client_v1";

/// Detect the version of WASI being used based on the import
/// namespaces.
///
/// A strict detection expects that all imports live in a single WASI
/// namespace. A non-strict detection expects that at least one WASI
/// namespace exists to detect the version. Note that the strict
/// detection is faster than the non-strict one.
pub fn get_wasi_version(module: &Module, strict: bool) -> Option<WasiVersion> {
    let mut imports = module.imports().functions().map(|f| f.module().to_owned());

    if strict {
        let first_module = imports.next()?;
        if imports.all(|module| module == first_module) {
            match first_module.as_str() {
                SNAPSHOT0_NAMESPACE => Some(WasiVersion::Snapshot0),
                SNAPSHOT1_NAMESPACE => Some(WasiVersion::Snapshot1),
                WASIX_32V1_NAMESPACE => Some(WasiVersion::Wasix32v1),
                WASIX_64V1_NAMESPACE => Some(WasiVersion::Wasix64v1),
                _ => None,
            }
        } else {
            None
        }
    } else {
        // Check that at least a WASI namespace exists, and use the
        // first one in the list to detect the WASI version.
        imports.find_map(|module| match module.as_str() {
            SNAPSHOT0_NAMESPACE => Some(WasiVersion::Snapshot0),
            SNAPSHOT1_NAMESPACE => Some(WasiVersion::Snapshot1),
            WASIX_32V1_NAMESPACE => Some(WasiVersion::Wasix32v1),
            WASIX_64V1_NAMESPACE => Some(WasiVersion::Wasix64v1),
            _ => None,
        })
    }
}

/// Like [`get_wasi_version`] but detects multiple WASI versions in a single module.
/// Thus `strict` behaves differently in this function as multiple versions are
/// always supported. `strict` indicates whether non-WASI imports should trigger a
/// failure or be ignored.
pub fn get_wasi_versions(module: &Module, strict: bool) -> Option<BTreeSet<WasiVersion>> {
    let mut out = BTreeSet::new();
    let imports = module.imports().functions().map(|f| f.module().to_owned());

    let mut non_wasi_seen = false;
    for ns in imports {
        match ns.as_str() {
            SNAPSHOT0_NAMESPACE => {
                out.insert(WasiVersion::Snapshot0);
            }
            SNAPSHOT1_NAMESPACE => {
                out.insert(WasiVersion::Snapshot1);
            }
            WASIX_32V1_NAMESPACE => {
                out.insert(WasiVersion::Wasix32v1);
            }
            WASIX_64V1_NAMESPACE => {
                out.insert(WasiVersion::Wasix64v1);
            }
            WASIX_HTTP_V1_NAMESPACE => {
                out.insert(WasiVersion::Wasix64v1);
            }
            _ => {
                non_wasi_seen = true;
            }
        }
    }
    if strict && non_wasi_seen {
        None
    } else {
        Some(out)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn wasi_version_equality() {
        assert_eq!(WasiVersion::Snapshot0, WasiVersion::Snapshot0);
        assert_eq!(WasiVersion::Wasix64v1, WasiVersion::Wasix64v1);
        assert_eq!(WasiVersion::Wasix32v1, WasiVersion::Wasix32v1);
        assert_eq!(WasiVersion::Snapshot1, WasiVersion::Snapshot1);
        assert_eq!(WasiVersion::Snapshot1, WasiVersion::Latest);
        assert_eq!(WasiVersion::Latest, WasiVersion::Snapshot1);
        assert_eq!(WasiVersion::Latest, WasiVersion::Latest);
        assert!(WasiVersion::Wasix32v1 != WasiVersion::Wasix64v1);
        assert!(WasiVersion::Wasix64v1 != WasiVersion::Wasix32v1);
        assert!(WasiVersion::Snapshot1 != WasiVersion::Wasix64v1);
        assert!(WasiVersion::Wasix64v1 != WasiVersion::Snapshot1);
        assert!(WasiVersion::Snapshot1 != WasiVersion::Wasix32v1);
        assert!(WasiVersion::Wasix32v1 != WasiVersion::Snapshot1);
        assert!(WasiVersion::Snapshot0 != WasiVersion::Snapshot1);
        assert!(WasiVersion::Snapshot1 != WasiVersion::Snapshot0);
        assert!(WasiVersion::Snapshot0 != WasiVersion::Latest);
        assert!(WasiVersion::Latest != WasiVersion::Snapshot0);
        assert!(WasiVersion::Snapshot0 != WasiVersion::Latest);
        assert!(WasiVersion::Latest != WasiVersion::Snapshot0);
        assert!(WasiVersion::Wasix32v1 != WasiVersion::Latest);
        assert!(WasiVersion::Wasix64v1 != WasiVersion::Latest);
    }

    #[test]
    fn wasi_version_ordering() {
        assert!(WasiVersion::Snapshot0 <= WasiVersion::Snapshot0);
        assert!(WasiVersion::Snapshot1 <= WasiVersion::Snapshot1);
        assert!(WasiVersion::Wasix32v1 <= WasiVersion::Wasix32v1);
        assert!(WasiVersion::Wasix64v1 <= WasiVersion::Wasix64v1);
        assert!(WasiVersion::Latest <= WasiVersion::Latest);
        assert!(WasiVersion::Snapshot0 >= WasiVersion::Snapshot0);
        assert!(WasiVersion::Snapshot1 >= WasiVersion::Snapshot1);
        assert!(WasiVersion::Wasix32v1 >= WasiVersion::Wasix32v1);
        assert!(WasiVersion::Wasix64v1 >= WasiVersion::Wasix64v1);
        assert!(WasiVersion::Latest >= WasiVersion::Latest);

        assert!(WasiVersion::Snapshot0 < WasiVersion::Latest);
        assert!(WasiVersion::Snapshot0 < WasiVersion::Wasix32v1);
        assert!(WasiVersion::Snapshot0 < WasiVersion::Wasix64v1);
        assert!(WasiVersion::Snapshot0 < WasiVersion::Snapshot1);
        assert!(WasiVersion::Latest > WasiVersion::Snapshot0);
        assert!(WasiVersion::Wasix32v1 > WasiVersion::Snapshot0);
        assert!(WasiVersion::Wasix64v1 > WasiVersion::Snapshot0);
        assert!(WasiVersion::Snapshot1 > WasiVersion::Snapshot0);

        assert!(WasiVersion::Snapshot1 < WasiVersion::Wasix32v1);
        assert!(WasiVersion::Snapshot1 < WasiVersion::Wasix64v1);
        assert!(WasiVersion::Wasix32v1 > WasiVersion::Snapshot1);
        assert!(WasiVersion::Wasix64v1 > WasiVersion::Snapshot1);

        assert!(WasiVersion::Wasix32v1 < WasiVersion::Latest);
        assert!(WasiVersion::Wasix32v1 > WasiVersion::Snapshot1);
        assert!(WasiVersion::Wasix64v1 < WasiVersion::Latest);
        assert!(WasiVersion::Wasix64v1 > WasiVersion::Snapshot1);
        assert!(WasiVersion::Latest > WasiVersion::Wasix32v1);
        assert!(WasiVersion::Snapshot1 < WasiVersion::Wasix32v1);
        assert!(WasiVersion::Latest > WasiVersion::Wasix64v1);
        assert!(WasiVersion::Snapshot1 < WasiVersion::Wasix32v1);

        assert!(WasiVersion::Wasix32v1 < WasiVersion::Wasix64v1);
        assert!(WasiVersion::Wasix64v1 > WasiVersion::Wasix32v1);
    }
}
