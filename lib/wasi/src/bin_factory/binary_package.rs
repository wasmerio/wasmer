use std::{
    any::Any,
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, RwLock},
};

use derivative::*;
use wasmer_vfs::{FileSystem, TmpFileSystem};
use wasmer_wasi_types::wasi::Snapshot0Clockid;

use crate::{syscalls::platform_clock_time_get, utils::hash_sha256};

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
            self.hash = Some(hash_sha256(self.atom.as_ref()));
        }
        let hash = self.hash.as_ref().unwrap();
        hash.as_str()
    }
}

#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct BinaryPackage {
    pub module: wasmer::Module,

    pub package_name: Cow<'static, str>,
    pub when_cached: Option<u128>,
    pub ownership: Option<Arc<dyn Any + Send + Sync + 'static>>,
    #[derivative(Debug = "ignore")]
    pub entry: Cow<'static, [u8]>,
    pub hash: String,
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
    pub fn new(package_name: &str, entry: Cow<'static, [u8]>, module: wasmer::Module) -> Self {
        let now = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000).unwrap() as u128;
        let (package_name, version) = match package_name.split_once('@') {
            Some((a, b)) => (a.to_string(), b.to_string()),
            None => (package_name.to_string(), "1.0.0".to_string()),
        };
        let module_memory_footprint = entry.len();
        let hash = crate::utils::hash_sha256(&entry);

        Self {
            module,
            hash,
            package_name: package_name.into(),
            when_cached: Some(now),
            ownership: None,
            entry,
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
            module_memory_footprint: entry.len() as u64,
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
        entry: Cow<'a, [u8]>,
        module: wasmer::Module,
        ownership: Arc<T>,
    ) -> Self
    where
        T: 'static,
    {
        let ownership: Arc<dyn Any> = ownership;
        let mut ret = Self::new(package_name, std::mem::transmute(entry), module);
        ret.ownership = Some(std::mem::transmute(ownership));
        ret
    }
}

// #[derive(Clone, Debug)]
// pub struct BinaryPackageConfigWebc {
//     pub uses: Vec<String>,
// }

// #[derive(Clone, Debug)]
// pub struct BinaryPackageConfig {
//     pub envs: HashMap<String, String>,
//     pub webc: Option<BinaryPackageConfigWebc>,
// }

// #[derive(Clone, Debug)]
// pub struct BinaryPackage {
//     // TODO: use custom Sha256 hash type to save memory / increase perf
//     pub module_hash: String,
//     pub module_size_bytes: u64,
//     pub module: wasmer::Module,

//     pub dependencies: Vec<String>,

//     pub commands: Arc<Vec<BinaryPackageCommand>>,

//     pub webc: Option<BinaryPackageWebc>,
// }

// #[derive(Clone, Debug)]
// pub struct BinaryPackageWebc {
//     // TODO: use custom Sha256 hash type to save memory / increase perf
//     pub hash: String,
//     pub name: Option<String>,
//     pub version: Option<String>,
//     pub uri: Option<String>,
//     pub atom: String,
//     pub fs_size_bytes: u64,
//     pub filesystem: Option<Arc<dyn FileSystem + Send + Sync + 'static>>,
//     pub webc: Arc<webc::WebCMmap>,
// }
