use std::{
    any::Any,
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
};

use derivative::*;
use wasmer_vfs::{FileSystem, TmpFileSystem};
use wasmer_wasi_types::wasi::Snapshot0Clockid;

use super::hash_of_binary;
use crate::syscalls::platform_clock_time_get;

#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct BinaryPackageCommand {
    pub name: String,
    #[derivative(Debug = "ignore")]
    pub atom: Cow<'static, [u8]>,
    hash: Option<String>,
    pub ownership: Option<Arc<dyn Any + Send + Sync + 'static>>,
}

impl BinaryPackageCommand {
    pub fn new(name: String, atom: Cow<'static, [u8]>) -> Self {
        Self {
            name,
            ownership: None,
            hash: None,
            atom,
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
        ownership: Arc<T>,
    ) -> Self
    where
        T: 'static,
    {
        let ownership: Arc<dyn Any> = ownership;
        let mut ret = Self::new(name, std::mem::transmute(atom));
        ret.ownership = Some(std::mem::transmute(ownership));
        ret
    }

    pub fn hash(&mut self) -> &str {
        if self.hash.is_none() {
            self.hash = Some(hash_of_binary(self.atom.as_ref()));
        }
        let hash = self.hash.as_ref().unwrap();
        hash.as_str()
    }
}

#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct BinaryPackage {
    pub package_name: Cow<'static, str>,
    pub when_cached: Option<u128>,
    pub ownership: Option<Arc<dyn Any + Send + Sync + 'static>>,
    #[derivative(Debug = "ignore")]
    pub entry: Option<Cow<'static, [u8]>>,
    pub hash: Arc<Mutex<Option<String>>>,
    pub wapm: Option<String>,
    pub base_dir: Option<String>,
    pub tmp_fs: TmpFileSystem,
    pub webc_fs: Option<Arc<dyn FileSystem + Send + Sync + 'static>>,
    pub webc_top_level_dirs: Vec<String>,
    pub mappings: Vec<String>,
    pub envs: HashMap<String, String>,
    pub commands: Arc<RwLock<Vec<BinaryPackageCommand>>>,
    pub uses: Vec<String>,
    pub version: Cow<'static, str>,
    pub module_memory_footprint: u64,
    pub file_system_memory_footprint: u64,
}

impl BinaryPackage {
    pub fn new(package_name: &str, entry: Option<Cow<'static, [u8]>>) -> Self {
        let now = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000).unwrap() as u128;
        let (package_name, version) = match package_name.split_once('@') {
            Some((a, b)) => (a.to_string(), b.to_string()),
            None => (package_name.to_string(), "1.0.0".to_string()),
        };
        let module_memory_footprint = entry.as_ref().map(|a| a.len()).unwrap_or_default() as u64;
        Self {
            package_name: package_name.into(),
            when_cached: Some(now),
            ownership: None,
            entry,
            hash: Arc::new(Mutex::new(None)),
            wapm: None,
            base_dir: None,
            tmp_fs: TmpFileSystem::new(),
            webc_fs: None,
            webc_top_level_dirs: Default::default(),
            mappings: Vec::new(),
            envs: HashMap::default(),
            commands: Arc::new(RwLock::new(Vec::new())),
            uses: Vec::new(),
            version: version.into(),
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
        entry: Option<Cow<'a, [u8]>>,
        ownership: Arc<T>,
    ) -> Self
    where
        T: 'static,
    {
        let ownership: Arc<dyn Any> = ownership;
        let mut ret = Self::new(package_name, entry.map(|a| std::mem::transmute(a)));
        ret.ownership = Some(std::mem::transmute(ownership));
        ret
    }

    pub fn hash(&self) -> String {
        let mut hash = self.hash.lock().unwrap();
        if hash.is_none() {
            if let Some(entry) = self.entry.as_ref() {
                hash.replace(hash_of_binary(entry.as_ref()));
            } else {
                hash.replace(hash_of_binary(self.package_name.as_ref()));
            }
        }
        hash.as_ref().unwrap().clone()
    }
}
