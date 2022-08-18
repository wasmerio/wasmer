use std::{
    sync::{
        Arc, Mutex, RwLock
    },
    any::Any,
    borrow::Cow,
    collections::HashMap
};

use derivative::*;
use wasmer_vfs::FileSystem;
use wasmer_wasi_types::__WASI_CLOCK_MONOTONIC;
use crate::{fs::TmpFileSystem, syscalls::platform_clock_time_get};

use super::hash_of_binary;

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
            atom
        }
    }

    pub unsafe fn new_with_ownership<'a, T>(name: String, atom: Cow<'a, [u8]>, ownership: Arc<T>) -> Self
    where T: 'static
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
    pub when_cached: u128,
    pub ownership: Option<Arc<dyn Any + Send + Sync + 'static>>,
    #[derivative(Debug = "ignore")]
    pub entry: Cow<'static, [u8]>,
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
}

impl BinaryPackage {
    pub fn new(package_name: &str, entry: Cow<'static, [u8]>) -> Self {
        let now = platform_clock_time_get(__WASI_CLOCK_MONOTONIC, 1_000_000).unwrap() as u128;
        let (package_name, version) = match package_name.split_once("@") {
            Some((a, b)) => (a.to_string(), b.to_string()),
            None => (package_name.to_string(), "1.0.0".to_string())
        };
        Self {
            package_name: package_name.into(),
            when_cached: now,
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
        }
    }

    pub unsafe fn new_with_ownership<'a, T>(package_name: &str, entry: Cow<'a, [u8]>, ownership: Arc<T>) -> Self
    where T: 'static
    {
        let ownership: Arc<dyn Any> = ownership;
        let mut ret = Self::new(package_name, std::mem::transmute(entry));
        ret.ownership = Some(std::mem::transmute(ownership));
        ret
    }
    
    pub fn hash(&self) -> String {
        let mut hash = self.hash.lock().unwrap();
        if hash.is_none() {
            hash.replace(hash_of_binary(self.entry.as_ref()));
        }
        hash.as_ref().unwrap().clone()
    }
}
