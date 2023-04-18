use std::{
    any::Any,
    borrow::Cow,
    sync::{Arc, Mutex, RwLock},
};

use derivative::*;
use once_cell::sync::OnceCell;
use virtual_fs::FileSystem;
use wasmer_wasix_types::wasi::Snapshot0Clockid;
use webc::compat::SharedBytes;

use super::hash_of_binary;
use crate::syscalls::platform_clock_time_get;

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

    /// Hold on to some arbitrary data for the lifetime of this binary pacakge.
    ///
    /// # Safety
    ///
    /// Must ensure that the atom data will be safe to use as long as the provided
    /// ownership handle stays alive.
    pub unsafe fn new_with_ownership<'a, T>(
        name: String,
        atom: Cow<'a, [u8]>,
        _ownership: Arc<T>,
    ) -> Self
    where
        T: 'static,
    {
        Self::new(name, atom.to_vec().into())
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn atom(&self) -> &[u8] {
        &self.atom
    }

    pub fn hash(&self) -> &str {
        self.hash.get_or_init(|| hash_of_binary(self.atom()))
    }
}

#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct BinaryPackage {
    pub package_name: String,
    pub when_cached: Option<u128>,
    pub ownership: Option<Arc<dyn Any + Send + Sync + 'static>>,
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
    pub fn new(package_name: &str, entry: Option<SharedBytes>) -> Self {
        let now = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000).unwrap() as u128;
        let (package_name, version) = match package_name.split_once('@') {
            Some((a, b)) => (a.to_string(), b.to_string()),
            None => (package_name.to_string(), "1.0.0".to_string()),
        };
        let module_memory_footprint = entry.as_ref().map(|a| a.len()).unwrap_or_default() as u64;
        Self {
            package_name,
            when_cached: Some(now),
            ownership: None,
            entry,
            hash: Arc::new(Mutex::new(None)),
            webc_fs: None,
            commands: Arc::new(RwLock::new(Vec::new())),
            uses: Vec::new(),
            version,
            module_memory_footprint,
            file_system_memory_footprint: 0,
        }
    }

    /// Hold on to some arbitrary data for the lifetime of this binary pacakge.
    ///
    /// # Safety
    ///
    /// Must ensure that the entry data will be safe to use as long as the provided
    /// ownership handle stays alive.
    pub unsafe fn new_with_ownership<'a, T>(
        package_name: &str,
        entry: Option<SharedBytes>,
        ownership: Arc<T>,
    ) -> Self
    where
        T: 'static,
    {
        let ownership: Arc<dyn Any> = ownership;
        let mut ret = Self::new(package_name, entry);
        ret.ownership = Some(std::mem::transmute(ownership));
        ret
    }

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
