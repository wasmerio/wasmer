#![allow(dead_code)]
#![allow(unused)]
use wasmer_vfs::*;
use std::borrow::Cow;
use std::ops::Add;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};

#[derive(Debug)]
pub struct MountPoint {
    pub path: String,
    pub name: String,
    pub fs: Option<Arc<Box<dyn FileSystem>>>,
    pub weak_fs: Weak<Box<dyn FileSystem>>,
    pub temp_holding: Arc<Mutex<Option<Arc<Box<dyn FileSystem>>>>>,
    pub should_sanitize: bool,
    pub new_path: Option<String>,
}

impl Clone for MountPoint {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            name: self.name.clone(),
            fs: None,
            weak_fs: self.weak_fs.clone(),
            temp_holding: self.temp_holding.clone(),
            should_sanitize: self.should_sanitize,
            new_path: self.new_path.clone(),
        }
    }
}

impl MountPoint {
    pub fn fs(&self) -> Option<Arc<Box<dyn FileSystem>>> {
        match &self.fs {
            Some(a) => Some(a.clone()),
            None => self.weak_fs.upgrade(),
        }
    }

    fn solidify(&mut self) {
        if self.fs.is_none() {
            self.fs = self.weak_fs.upgrade();
        }
        {
            let mut guard = self.temp_holding.lock().unwrap();
            let fs = guard.take();
            if self.fs.is_none() {
                self.fs = fs;
            }
        }
    }

    fn strong(&self) -> Option<StrongMountPoint> {
        match self.fs() {
            Some(fs) => Some(StrongMountPoint {
                path: self.path.clone(),
                name: self.name.clone(),
                fs,
                should_sanitize: self.should_sanitize,
                new_path: self.new_path.clone(),
            }),
            None => None,
        }
    }
}

#[derive(Debug)]
pub struct StrongMountPoint {
    pub path: String,
    pub name: String,
    pub fs: Arc<Box<dyn FileSystem>>,
    pub should_sanitize: bool,
    pub new_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UnionFileSystem {
    pub mounts: Vec<MountPoint>,
}

impl UnionFileSystem {
    pub fn new() -> UnionFileSystem {
        UnionFileSystem {
            mounts: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        self.mounts.clear();
    }
}

impl UnionFileSystem {
    pub fn mount(
        &mut self,
        name: &str,
        path: &str,
        should_sanitize: bool,
        fs: Box<dyn FileSystem>,
        new_path: Option<&str>,
    ) {
        self.unmount(path);
        let mut path = path.to_string();
        if path.starts_with("/") == false {
            path.insert(0, '/');
        }
        if path.ends_with("/") == false {
            path += "/";
        }
        let new_path = new_path.map(|new_path| {
            let mut new_path = new_path.to_string();
            if new_path.ends_with("/") == false {
                new_path += "/";
            }
            new_path
        });
        let fs = Arc::new(fs);

        let mount = MountPoint {
            path,
            name: name.to_string(),
            fs: None,
            weak_fs: Arc::downgrade(&fs),
            temp_holding: Arc::new(Mutex::new(Some(fs.clone()))),
            should_sanitize,
            new_path,
        };

        self.mounts.push(mount);
    }

    pub fn unmount(&mut self, path: &str) {
        let path1 = path.to_string();
        let mut path2 = path1.clone();
        if path2.starts_with("/") == false {
            path2.insert(0, '/');
        }
        let mut path3 = path2.clone();
        if path3.ends_with("/") == false {
            path3.push_str("/")
        }
        if path2.ends_with("/") {
            path2 = (&path2[..(path2.len() - 1)]).to_string();
        }

        self.mounts
            .retain(|mount| mount.path != path2 && mount.path != path3);
    }

    fn read_dir_internal(&self, path: &Path) -> Result<ReadDir> {
        let path = path.to_string_lossy();

        let mut ret = None;
        for (path, mount) in filter_mounts(&self.mounts, path.as_ref()) {
            match mount.fs.read_dir(Path::new(path.as_str())) {
                Ok(dir) => {
                    if ret.is_none() {
                        ret = Some(Vec::new());
                    }
                    let ret = ret.as_mut().unwrap();
                    for sub in dir {
                        if let Ok(sub) = sub {
                            ret.push(sub);
                        }
                    }
                }
                Err(err) => {
                    debug!("failed to read dir - {}", err);
                }
            }
        }

        match ret {
            Some(ret) => Ok(ReadDir::new(ret)),
            None => Err(FsError::EntryNotFound),
        }
    }

    pub fn sanitize(mut self) -> Self {
        self.solidify();
        self.mounts.retain(|mount| mount.should_sanitize == false);
        self
    }

    pub fn solidify(&mut self) {
        for mount in self.mounts.iter_mut() {
            mount.solidify();
        }
    }
}

impl FileSystem for UnionFileSystem {
    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        debug!("read_dir: path={}", path.display());
        self.read_dir_internal(path)
    }
    fn create_dir(&self, path: &Path) -> Result<()> {
        debug!("create_dir: path={}", path.display());

        if self.read_dir_internal(path).is_ok() {
            //return Err(FsError::AlreadyExists);
            return Ok(());
        }

        let path = path.to_string_lossy();
        let mut ret_error = FsError::EntryNotFound;
        for (path, mount) in filter_mounts(&self.mounts, path.as_ref()) {
            match mount.fs.create_dir(Path::new(path.as_str())) {
                Ok(ret) => {
                    return Ok(ret);
                }
                Err(err) => {
                    ret_error = err;
                }
            }
        }
        Err(ret_error)
    }
    fn remove_dir(&self, path: &Path) -> Result<()> {
        debug!("remove_dir: path={}", path.display());
        let mut ret_error = FsError::EntryNotFound;
        let path = path.to_string_lossy();
        for (path, mount) in filter_mounts(&self.mounts, path.as_ref()) {
            match mount.fs.remove_dir(Path::new(path.as_str())) {
                Ok(ret) => {
                    return Ok(ret);
                }
                Err(err) => {
                    ret_error = err;
                }
            }
        }
        Err(ret_error)
    }
    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        debug!("rename: from={} to={}", from.display(), to.display());
        let mut ret_error = FsError::EntryNotFound;
        let from = from.to_string_lossy();
        let to = to.to_string_lossy();
        for (path, mount) in filter_mounts(&self.mounts, from.as_ref()) {
            let mut to = if to.starts_with(mount.path.as_str()) {
                (&to[mount.path.len()..]).to_string()
            } else {
                ret_error = FsError::UnknownError;
                continue;
            };
            if to.starts_with("/") == false {
                to = format!("/{}", to);
            }
            match mount.fs.rename(Path::new(from.as_ref()), Path::new(to.as_str())) {
                Ok(ret) => {
                    trace!("rename ok");
                    return Ok(ret);
                }
                Err(err) => {
                    trace!("rename error (from={}, to={}) - {}", from, to, err);            
                    ret_error = err;
                }
            }
        }
        trace!("rename failed - {}", ret_error);
        Err(ret_error)
    }
    fn metadata(&self, path: &Path) -> Result<Metadata> {
        debug!("metadata: path={}", path.display());
        let mut ret_error = FsError::EntryNotFound;
        let path = path.to_string_lossy();
        for (path, mount) in filter_mounts(&self.mounts, path.as_ref()) {
            match mount.fs.metadata(Path::new(path.as_str())) {
                Ok(ret) => {
                    return Ok(ret);
                }
                Err(err) => {
                    // This fixes a bug when attempting to create the directory /usr when it does not exist
                    // on the x86 version of memfs
                    // TODO: patch wasmer_vfs and remove
                    if let FsError::NotAFile = &err {
                        ret_error = FsError::EntryNotFound;
                    } else {
                        debug!("metadata failed: (path={}) - {}", path, err);
                        ret_error = err;
                    }
                }
            }
        }
        Err(ret_error)
    }
    fn symlink_metadata(&self, path: &Path) -> Result<Metadata> {
        debug!("symlink_metadata: path={}", path.display());
        let mut ret_error = FsError::EntryNotFound;
        let path = path.to_string_lossy();
        for (path_inner, mount) in filter_mounts(&self.mounts, path.as_ref()) {
            match mount.fs.symlink_metadata(Path::new(path_inner.as_str())) {
                Ok(ret) => {
                    return Ok(ret);
                }
                Err(err) => {
                    // This fixes a bug when attempting to create the directory /usr when it does not exist
                    // on the x86 version of memfs
                    // TODO: patch wasmer_vfs and remove
                    if let FsError::NotAFile = &err {
                        ret_error = FsError::EntryNotFound;
                    } else {
                        debug!("metadata failed: (path={}) - {}", path, err);
                        ret_error = err;
                    }
                }
            }
        }
        debug!("symlink_metadata: failed={}", ret_error);
        Err(ret_error)
    }
    fn remove_file(&self, path: &Path) -> Result<()> {
        debug!("remove_file: path={}", path.display());
        let mut ret_error = FsError::EntryNotFound;
        let path = path.to_string_lossy();
        for (path, mount) in filter_mounts(&self.mounts, path.as_ref()) {
            match mount.fs.remove_file(Path::new(path.as_str())) {
                Ok(ret) => {
                    return Ok(ret);
                }
                Err(err) => {
                    ret_error = err;
                }
            }
        }
        Err(ret_error)
    }
    fn new_open_options(&self) -> OpenOptions {
        let opener = Box::new(UnionFileOpener {
            mounts: self.mounts.clone(),
        });
        OpenOptions::new(opener)
    }
}

fn filter_mounts(
    mounts: &Vec<MountPoint>,
    mut target: &str,
) -> impl Iterator<Item = (String, StrongMountPoint)> {
    let mut biggest_path = 0usize;
    let mut ret = Vec::new();
    for mount in mounts.iter().rev() {
        let mut test_mount_path1 = mount.path.clone();
        if test_mount_path1.ends_with("/") == false {
            test_mount_path1.push_str("/");
        }

        let mut test_mount_path2 = mount.path.clone();
        if test_mount_path2.ends_with("/") == true {
            test_mount_path2 = test_mount_path2[..(test_mount_path2.len() - 1)].to_string();
        }

        if target == test_mount_path1 || target == test_mount_path2 {
            if let Some(mount) = mount.strong() {
                biggest_path = biggest_path.max(mount.path.len());
                let mut path = "/".to_string();
                if let Some(ref np) = mount.new_path {
                    path = np.to_string();
                }
                ret.push((path, mount));
            }
        } else if target.starts_with(test_mount_path1.as_str()) {
            if let Some(mount) = mount.strong() {
                biggest_path = biggest_path.max(mount.path.len());
                let path = &target[test_mount_path2.len()..];
                let mut path = path.to_string();
                if let Some(ref np) = mount.new_path {
                    path = format!("{}{}", np, &path[1..]);
                }
                ret.push((path, mount));
            }
        }
    }
    ret.retain(|(a, b)| b.path.len() >= biggest_path);
    ret.into_iter()
}

#[derive(Debug)]
pub struct UnionFileOpener {
    mounts: Vec<MountPoint>,
}

impl FileOpener for UnionFileOpener {
    fn open(
        &mut self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync>> {
        debug!("open: path={}", path.display());
        let mut ret_err = FsError::EntryNotFound;
        let path = path.to_string_lossy();
        if conf.create() || conf.create_new() {
            for (path, mount) in filter_mounts(&self.mounts, path.as_ref()) {
                if let Ok(mut ret) = mount
                    .fs
                    .new_open_options()
                    .truncate(conf.truncate())
                    .append(conf.append())
                    .read(conf.read())
                    .write(conf.write())
                    .open(path)
                {
                    if conf.create_new() {
                        ret.unlink();
                        continue;
                    }
                    return Ok(ret);
                }
            }
        }
        for (path, mount) in filter_mounts(&self.mounts, path.as_ref()) {
            match mount
                .fs
                .new_open_options()
                .options(conf.clone())
                .open(path)
            {
                Ok(ret) => return Ok(ret),
                Err(err) if ret_err == FsError::EntryNotFound => {
                    ret_err = err;
                }
                _ => {}
            }
        }
        Err(ret_err)
    }
}
