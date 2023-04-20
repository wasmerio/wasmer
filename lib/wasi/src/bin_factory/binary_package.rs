use std::sync::{Arc, Mutex, RwLock};

use derivative::*;
use once_cell::sync::OnceCell;
use virtual_fs::FileSystem;
use webc::compat::SharedBytes;

use super::hash_of_binary;

#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct BinaryPackageCommand {
    name: String,
    #[derivative(Debug = "ignore")]
    pub(crate) atom: SharedBytes,
    hash: OnceCell<String>,
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

    pub fn hash(&self) -> &str {
        self.hash.get_or_init(|| hash_of_binary(self.atom()))
    }
}

/// A WebAssembly package that has been loaded into memory.
///
/// You can crate a [`BinaryPackage`] using [`crate::bin_factory::ModuleCache`]
/// or [`crate::wapm::parse_static_webc()`].
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct BinaryPackage {
    pub package_name: String,
    pub when_cached: Option<u128>,
    #[derivative(Debug = "ignore")]
    pub entry: Option<SharedBytes>,
    pub hash: Arc<Mutex<Option<String>>>,
    pub webc_fs: Option<Arc<dyn FileSystem + Send + Sync + 'static>>,
    pub commands: Arc<RwLock<Vec<BinaryPackageCommand>>>,
    pub uses: Vec<String>,
    pub version: String,
    pub module_memory_footprint: u64,
    pub file_system_memory_footprint: u64,
}

impl BinaryPackage {
    pub fn hash(&self) -> String {
        let mut hash = self.hash.lock().unwrap();
        if hash.is_none() {
            if let Some(entry) = self.entry.as_ref() {
                hash.replace(hash_of_binary(entry.as_ref()));
            } else {
                hash.replace(hash_of_binary(&self.package_name));
            }
        }
        hash.as_ref().unwrap().clone()
    }
}
