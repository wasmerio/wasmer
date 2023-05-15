use std::sync::{Arc, RwLock};

use derivative::*;
use once_cell::sync::OnceCell;
use semver::Version;
use virtual_fs::FileSystem;
use webc::compat::SharedBytes;

use crate::runtime::module_cache::ModuleHash;

#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct BinaryPackageCommand {
    name: String,
    #[derivative(Debug = "ignore")]
    pub(crate) atom: SharedBytes,
    hash: OnceCell<ModuleHash>,
}

impl BinaryPackageCommand {
    pub fn new(name: String, atom: SharedBytes) -> Self {
        Self {
            name,
            atom,
            hash: OnceCell::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get a reference to this [`BinaryPackageCommand`]'s atom.
    ///
    /// The address of the returned slice is guaranteed to be stable and live as
    /// long as the [`BinaryPackageCommand`].
    pub fn atom(&self) -> &[u8] {
        &self.atom
    }

    pub fn hash(&self) -> &ModuleHash {
        self.hash.get_or_init(|| ModuleHash::sha256(self.atom()))
    }
}

/// A WebAssembly package that has been loaded into memory.
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct BinaryPackage {
    pub package_name: String,
    pub when_cached: Option<u128>,
    #[derivative(Debug = "ignore")]
    pub entry: Option<SharedBytes>,
    pub hash: OnceCell<ModuleHash>,
    pub webc_fs: Option<Arc<dyn FileSystem + Send + Sync + 'static>>,
    pub commands: Arc<RwLock<Vec<BinaryPackageCommand>>>,
    pub uses: Vec<String>,
    pub version: Version,
    pub module_memory_footprint: u64,
    pub file_system_memory_footprint: u64,
}

impl BinaryPackage {
    pub fn hash(&self) -> ModuleHash {
        *self.hash.get_or_init(|| {
            if let Some(entry) = self.entry.as_ref() {
                ModuleHash::sha256(entry)
            } else {
                ModuleHash::sha256(self.package_name.as_bytes())
            }
        })
    }
}
