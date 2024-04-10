//! Another implementation of the union that uses paths,
//! its not as simple as TmpFs. not currently used but was used by
//! the previoulsy implementation of Deploy - now using TmpFs

use crate::*;

use std::{
    path::Path,
    sync::{Arc, Mutex, Weak},
};

use tracing::{debug, trace};

pub type TempHolding = Arc<Mutex<Option<Arc<Box<dyn FileSystem>>>>>;

#[derive(Debug)]
pub struct MountPoint {
    pub path: String,
    pub name: String,
    pub fs: Option<Arc<Box<dyn FileSystem>>>,
    pub weak_fs: Weak<Box<dyn FileSystem>>,
    pub temp_holding: TempHolding,
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

    /// Tries to recover the internal `Weak<dyn FileSystem>` to a `Arc<dyn FileSystem>`
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

    /// Returns a strong-referenced copy of the internal `Arc<dyn FileSystem>`
    fn strong(&self) -> Option<StrongMountPoint> {
        self.fs().map(|fs| StrongMountPoint {
            path: self.path.clone(),
            name: self.name.clone(),
            fs,
            should_sanitize: self.should_sanitize,
            new_path: self.new_path.clone(),
        })
    }
}

/// A `strong` mount point holds a strong `Arc` reference to the filesystem
/// mounted at path `path`.
#[derive(Debug)]
pub struct StrongMountPoint {
    pub path: String,
    pub name: String,
    pub fs: Arc<Box<dyn FileSystem>>,
    pub should_sanitize: bool,
    pub new_path: Option<String>,
}

/// Allows different filesystems of different types
/// to be mounted at various mount points
#[derive(Debug, Default, Clone)]
pub struct UnionFileSystem {
    pub mounts: Vec<MountPoint>,
}

impl UnionFileSystem {
    pub fn new() -> Self {
        Self::default()
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
        if !path.starts_with('/') {
            path.insert(0, '/');
        }
        if !path.ends_with('/') {
            path += "/";
        }
        let new_path = new_path.map(|new_path| {
            let mut new_path = new_path.to_string();
            if !new_path.ends_with('/') {
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
        let mut path2 = path1;
        if !path2.starts_with('/') {
            path2.insert(0, '/');
        }
        let mut path3 = path2.clone();
        if !path3.ends_with('/') {
            path3.push('/')
        }
        if path2.ends_with('/') {
            path2 = (path2[..(path2.len() - 1)]).to_string();
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
                    for sub in dir.flatten() {
                        ret.push(sub);
                    }
                }
                Err(err) => {
                    debug!("failed to read dir - {}", err);
                }
            }
        }

        match ret {
            Some(mut ret) => {
                ret.sort_by(|a, b| match (a.metadata.as_ref(), b.metadata.as_ref()) {
                    (Ok(a), Ok(b)) => a.modified.cmp(&b.modified),
                    _ => std::cmp::Ordering::Equal,
                });
                Ok(ReadDir::new(ret))
            }
            None => Err(FsError::EntryNotFound),
        }
    }

    /// Deletes all mount points that do not have `sanitize` set in the options
    pub fn sanitize(mut self) -> Self {
        self.solidify();
        self.mounts.retain(|mount| !mount.should_sanitize);
        self
    }

    /// Tries to recover the internal `Weak<dyn FileSystem>` to a `Arc<dyn FileSystem>`
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
        if path.parent().is_none() {
            return Err(FsError::BaseNotDirectory);
        }
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
        if path.parent().is_none() {
            return Err(FsError::BaseNotDirectory);
        }
        // https://github.com/rust-lang/rust/issues/86442
        // DirectoryNotEmpty is not implemented consistently
        if path.is_dir() && self.read_dir(path).map(|s| !s.is_empty()).unwrap_or(false) {
            return Err(FsError::DirectoryNotEmpty);
        }
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
    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async {
            println!("rename: from={} to={}", from.display(), to.display());
            if from.parent().is_none() {
                return Err(FsError::BaseNotDirectory);
            }
            if to.parent().is_none() {
                return Err(FsError::BaseNotDirectory);
            }
            let mut ret_error = FsError::EntryNotFound;
            let from = from.to_string_lossy();
            let to = to.to_string_lossy();
            #[cfg(target_os = "windows")]
            let to = to.replace('\\', "/");
            for (path, mount) in filter_mounts(&self.mounts, from.as_ref()) {
                let mut to = if to.starts_with(mount.path.as_str()) {
                    (to[mount.path.len()..]).to_string()
                } else {
                    ret_error = FsError::UnknownError;
                    continue;
                };
                if !to.starts_with('/') {
                    to = format!("/{}", to);
                }
                match mount
                    .fs
                    .rename(Path::new(&path), Path::new(to.as_str()))
                    .await
                {
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
        })
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
                    // TODO: patch virtual-fs and remove
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
                    // TODO: patch virtual-fs and remove
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
        println!("remove_file: path={}", path.display());
        let mut ret_error = FsError::EntryNotFound;
        let path = path.to_string_lossy();
        for (path, mount) in filter_mounts(&self.mounts, path.as_ref()) {
            match mount.fs.remove_file(Path::new(path.as_str())) {
                Ok(ret) => {
                    return Ok(ret);
                }
                Err(err) => {
                    println!("returning error {err:?}");
                    ret_error = err;
                }
            }
        }
        Err(ret_error)
    }
    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(self)
    }
}

fn filter_mounts(
    mounts: &[MountPoint],
    target: &str,
) -> impl Iterator<Item = (String, StrongMountPoint)> {
    // On Windows, Path might use \ instead of /, wich mill messup the matching of mount points
    #[cfg(target_os = "windows")]
    let target = target.replace('\\', "/");

    let mut biggest_path = 0usize;
    let mut ret = Vec::new();
    for mount in mounts.iter().rev() {
        let mut test_mount_path1 = mount.path.clone();
        if !test_mount_path1.ends_with('/') {
            test_mount_path1.push('/');
        }

        let mut test_mount_path2 = mount.path.clone();
        if test_mount_path2.ends_with('/') {
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
    ret.retain(|(_, b)| b.path.len() >= biggest_path);
    ret.into_iter()
}

impl FileOpener for UnionFileSystem {
    fn open(
        &self,
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
                        ret.unlink().ok();
                        continue;
                    }
                    return Ok(ret);
                }
            }
        }
        for (path, mount) in filter_mounts(&self.mounts, path.as_ref()) {
            match mount.fs.new_open_options().options(conf.clone()).open(path) {
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tokio::io::AsyncWriteExt;

    use crate::{mem_fs, ops, FileSystem as FileSystemTrait, FsError, UnionFileSystem};

    fn gen_filesystem() -> UnionFileSystem {
        let mut union = UnionFileSystem::new();
        // fs.mount("/", Box::new(mem_fs::FileSystem::default()));
        let a = mem_fs::FileSystem::default();
        let b = mem_fs::FileSystem::default();
        let c = mem_fs::FileSystem::default();
        let d = mem_fs::FileSystem::default();
        let e = mem_fs::FileSystem::default();
        let f = mem_fs::FileSystem::default();
        let g = mem_fs::FileSystem::default();
        let h = mem_fs::FileSystem::default();

        union.mount("mem_fs_1", "/test_new_filesystem", false, Box::new(a), None);
        union.mount("mem_fs_2", "/test_create_dir", false, Box::new(b), None);
        union.mount("mem_fs_3", "/test_remove_dir", false, Box::new(c), None);
        union.mount("mem_fs_4", "/test_rename", false, Box::new(d), None);
        union.mount("mem_fs_5", "/test_metadata", false, Box::new(e), None);
        union.mount("mem_fs_6", "/test_remove_file", false, Box::new(f), None);
        union.mount("mem_fs_6", "/test_readdir", false, Box::new(g), None);
        union.mount("mem_fs_6", "/test_canonicalize", false, Box::new(h), None);

        union
    }

    #[tokio::test]
    async fn test_new_filesystem() {
        let fs = gen_filesystem();
        assert!(
            fs.read_dir(Path::new("/test_new_filesystem")).is_ok(),
            "hostfs can read root"
        );
        let mut file_write = fs
            .new_open_options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(Path::new("/test_new_filesystem/foo2.txt"))
            .unwrap();
        file_write.write_all(b"hello").await.unwrap();
        let _ = std::fs::remove_file("/test_new_filesystem/foo2.txt");
    }

    #[tokio::test]
    async fn test_create_dir() {
        let fs = gen_filesystem();

        assert_eq!(
            fs.create_dir(Path::new("/")),
            Err(FsError::BaseNotDirectory),
            "creating a directory that has no parent",
        );

        let _ = fs_extra::remove_items(&["/test_create_dir"]);

        assert_eq!(
            fs.create_dir(Path::new("/test_create_dir")),
            Ok(()),
            "creating a directory",
        );

        assert_eq!(
            fs.create_dir(Path::new("/test_create_dir/foo")),
            Ok(()),
            "creating a directory",
        );

        let cur_dir = read_dir_names(&fs, "/test_create_dir");

        if !cur_dir.contains(&"foo".to_string()) {
            panic!("cur_dir does not contain foo: {:#?}", cur_dir);
        }

        assert!(
            cur_dir.contains(&"foo".to_string()),
            "the root is updated and well-defined"
        );

        assert_eq!(
            fs.create_dir(Path::new("/test_create_dir/foo/bar")),
            Ok(()),
            "creating a sub-directory",
        );

        let foo_dir = read_dir_names(&fs, "/test_create_dir/foo");

        assert!(
            foo_dir.contains(&"bar".to_string()),
            "the foo directory is updated and well-defined"
        );

        let bar_dir = read_dir_names(&fs, "/test_create_dir/foo/bar");

        assert!(
            bar_dir.is_empty(),
            "the foo directory is updated and well-defined"
        );
        let _ = fs_extra::remove_items(&["/test_create_dir"]);
    }

    #[tokio::test]
    async fn test_remove_dir() {
        let fs = gen_filesystem();

        let _ = fs_extra::remove_items(&["/test_remove_dir"]);

        assert_eq!(
            fs.remove_dir(Path::new("/")),
            Err(FsError::BaseNotDirectory),
            "removing a directory that has no parent",
        );

        assert_eq!(
            fs.remove_dir(Path::new("/foo")),
            Err(FsError::EntryNotFound),
            "cannot remove a directory that doesn't exist",
        );

        assert_eq!(
            fs.create_dir(Path::new("/test_remove_dir")),
            Ok(()),
            "creating a directory",
        );

        assert_eq!(
            fs.create_dir(Path::new("/test_remove_dir/foo")),
            Ok(()),
            "creating a directory",
        );

        assert_eq!(
            fs.create_dir(Path::new("/test_remove_dir/foo/bar")),
            Ok(()),
            "creating a sub-directory",
        );

        assert!(
            read_dir_names(&fs, "/test_remove_dir/foo").contains(&"bar".to_string()),
            "./foo/bar exists"
        );

        assert_eq!(
            fs.remove_dir(Path::new("/test_remove_dir/foo")),
            Err(FsError::DirectoryNotEmpty),
            "removing a directory that has children",
        );

        assert_eq!(
            fs.remove_dir(Path::new("/test_remove_dir/foo/bar")),
            Ok(()),
            "removing a sub-directory",
        );

        assert_eq!(
            fs.remove_dir(Path::new("/test_remove_dir/foo")),
            Ok(()),
            "removing a directory",
        );

        assert!(
            !read_dir_names(&fs, "/test_remove_dir").contains(&"foo".to_string()),
            "the foo directory still exists"
        );

        let _ = fs_extra::remove_items(&["/test_remove_dir"]);
    }

    fn read_dir_names(fs: &dyn crate::FileSystem, path: &str) -> Vec<String> {
        fs.read_dir(Path::new(path))
            .unwrap()
            .filter_map(|entry| Some(entry.ok()?.file_name().to_str()?.to_string()))
            .collect::<Vec<_>>()
    }

    #[tokio::test]
    async fn test_rename() {
        let fs = gen_filesystem();

        let _ = fs_extra::remove_items(&["./test_rename"]);

        assert_eq!(
            fs.rename(Path::new("/"), Path::new("/bar")).await,
            Err(FsError::BaseNotDirectory),
            "renaming a directory that has no parent",
        );
        assert_eq!(
            fs.rename(Path::new("/foo"), Path::new("/")).await,
            Err(FsError::BaseNotDirectory),
            "renaming to a directory that has no parent",
        );

        assert_eq!(fs.create_dir(Path::new("/test_rename")), Ok(()));
        assert_eq!(fs.create_dir(Path::new("/test_rename/foo")), Ok(()));
        assert_eq!(fs.create_dir(Path::new("/test_rename/foo/qux")), Ok(()));

        assert_eq!(
            fs.rename(
                Path::new("/test_rename/foo"),
                Path::new("/test_rename/bar/baz")
            )
            .await,
            Err(FsError::EntryNotFound),
            "renaming to a directory that has parent that doesn't exist",
        );

        assert_eq!(fs.create_dir(Path::new("/test_rename/bar")), Ok(()));

        assert_eq!(
            fs.rename(Path::new("/test_rename/foo"), Path::new("/test_rename/bar"))
                .await,
            Ok(()),
            "renaming to a directory that has parent that exists",
        );

        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(Path::new("/test_rename/bar/hello1.txt")),
                Ok(_),
            ),
            "creating a new file (`hello1.txt`)",
        );
        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(Path::new("/test_rename/bar/hello2.txt")),
                Ok(_),
            ),
            "creating a new file (`hello2.txt`)",
        );

        let cur_dir = read_dir_names(&fs, "/test_rename");

        assert!(
            !cur_dir.contains(&"foo".to_string()),
            "the foo directory still exists"
        );

        assert!(
            cur_dir.contains(&"bar".to_string()),
            "the bar directory still exists"
        );

        let bar_dir = read_dir_names(&fs, "/test_rename/bar");

        if !bar_dir.contains(&"qux".to_string()) {
            println!("qux does not exist: {:?}", bar_dir)
        }

        let qux_dir = read_dir_names(&fs, "/test_rename/bar/qux");

        assert!(qux_dir.is_empty(), "the qux directory is empty");

        assert!(
            read_dir_names(&fs, "/test_rename/bar").contains(&"hello1.txt".to_string()),
            "the /bar/hello1.txt file exists"
        );

        assert!(
            read_dir_names(&fs, "/test_rename/bar").contains(&"hello2.txt".to_string()),
            "the /bar/hello2.txt file exists"
        );

        assert_eq!(
            fs.create_dir(Path::new("/test_rename/foo")),
            Ok(()),
            "create ./foo again",
        );

        assert_eq!(
            fs.rename(
                Path::new("/test_rename/bar/hello2.txt"),
                Path::new("/test_rename/foo/world2.txt")
            )
            .await,
            Ok(()),
            "renaming (and moving) a file",
        );

        assert_eq!(
            fs.rename(
                Path::new("/test_rename/foo"),
                Path::new("/test_rename/bar/baz")
            )
            .await,
            Ok(()),
            "renaming a directory",
        );

        assert_eq!(
            fs.rename(
                Path::new("/test_rename/bar/hello1.txt"),
                Path::new("/test_rename/bar/world1.txt")
            )
            .await,
            Ok(()),
            "renaming a file (in the same directory)",
        );

        assert!(
            read_dir_names(&fs, "/test_rename").contains(&"bar".to_string()),
            "./bar exists"
        );

        assert!(
            read_dir_names(&fs, "/test_rename/bar").contains(&"baz".to_string()),
            "/bar/baz exists"
        );
        assert!(
            !read_dir_names(&fs, "/test_rename").contains(&"foo".to_string()),
            "foo does not exist anymore"
        );
        assert!(
            read_dir_names(&fs, "/test_rename/bar/baz").contains(&"world2.txt".to_string()),
            "/bar/baz/world2.txt exists"
        );
        assert!(
            read_dir_names(&fs, "/test_rename/bar").contains(&"world1.txt".to_string()),
            "/bar/world1.txt (ex hello1.txt) exists"
        );
        assert!(
            !read_dir_names(&fs, "/test_rename/bar").contains(&"hello1.txt".to_string()),
            "hello1.txt was moved"
        );
        assert!(
            !read_dir_names(&fs, "/test_rename/bar").contains(&"hello2.txt".to_string()),
            "hello2.txt was moved"
        );
        assert!(
            read_dir_names(&fs, "/test_rename/bar/baz").contains(&"world2.txt".to_string()),
            "world2.txt was moved to the correct place"
        );

        let _ = fs_extra::remove_items(&["/test_rename"]);
    }

    #[tokio::test]
    async fn test_metadata() {
        use std::thread::sleep;
        use std::time::Duration;

        let fs = gen_filesystem();

        let _ = fs_extra::remove_items(&["/test_metadata"]);

        assert_eq!(fs.create_dir(Path::new("/test_metadata")), Ok(()));

        let root_metadata = fs.metadata(Path::new("/test_metadata")).unwrap();

        assert!(root_metadata.ft.dir);
        assert!(root_metadata.accessed == root_metadata.created);
        assert!(root_metadata.modified == root_metadata.created);
        assert!(root_metadata.modified > 0);

        assert_eq!(fs.create_dir(Path::new("/test_metadata/foo")), Ok(()));

        let foo_metadata = fs.metadata(Path::new("/test_metadata/foo"));
        assert!(foo_metadata.is_ok());
        let foo_metadata = foo_metadata.unwrap();

        assert!(foo_metadata.ft.dir);
        assert!(foo_metadata.accessed == foo_metadata.created);
        assert!(foo_metadata.modified == foo_metadata.created);
        assert!(foo_metadata.modified > 0);

        sleep(Duration::from_secs(3));

        assert_eq!(
            fs.rename(
                Path::new("/test_metadata/foo"),
                Path::new("/test_metadata/bar")
            )
            .await,
            Ok(())
        );

        let bar_metadata = fs.metadata(Path::new("/test_metadata/bar")).unwrap();
        assert!(bar_metadata.ft.dir);
        assert!(bar_metadata.accessed == foo_metadata.accessed);
        assert!(bar_metadata.created == foo_metadata.created);
        assert!(bar_metadata.modified > foo_metadata.modified);

        let root_metadata = fs.metadata(Path::new("/test_metadata/bar")).unwrap();
        assert!(
            root_metadata.modified > foo_metadata.modified,
            "the parent modified time was updated"
        );

        let _ = fs_extra::remove_items(&["/test_metadata"]);
    }

    #[tokio::test]
    async fn test_remove_file() {
        let fs = gen_filesystem();

        let _ = fs_extra::remove_items(&["./test_remove_file"]);

        assert!(fs.create_dir(Path::new("/test_remove_file")).is_ok());

        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(Path::new("/test_remove_file/foo.txt")),
                Ok(_)
            ),
            "creating a new file",
        );

        assert!(read_dir_names(&fs, "/test_remove_file").contains(&"foo.txt".to_string()));

        assert_eq!(
            fs.remove_file(Path::new("/test_remove_file/foo.txt")),
            Ok(()),
            "removing a file that exists",
        );

        assert!(!read_dir_names(&fs, "/test_remove_file").contains(&"foo.txt".to_string()));

        assert_eq!(
            fs.remove_file(Path::new("/test_remove_file/foo.txt")),
            Err(FsError::EntryNotFound),
            "removing a file that doesn't exists",
        );

        let _ = fs_extra::remove_items(&["./test_remove_file"]);
    }

    #[tokio::test]
    async fn test_readdir() {
        let fs = gen_filesystem();

        assert_eq!(
            fs.create_dir(Path::new("/test_readdir/foo")),
            Ok(()),
            "creating `foo`"
        );
        assert_eq!(
            fs.create_dir(Path::new("/test_readdir/foo/sub")),
            Ok(()),
            "creating `sub`"
        );
        assert_eq!(
            fs.create_dir(Path::new("/test_readdir/bar")),
            Ok(()),
            "creating `bar`"
        );
        assert_eq!(
            fs.create_dir(Path::new("/test_readdir/baz")),
            Ok(()),
            "creating `bar`"
        );
        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(Path::new("/test_readdir/a.txt")),
                Ok(_)
            ),
            "creating `a.txt`",
        );
        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(Path::new("/test_readdir/b.txt")),
                Ok(_)
            ),
            "creating `b.txt`",
        );

        println!("fs: {:?}", fs);

        let readdir = fs.read_dir(Path::new("/test_readdir"));

        assert!(readdir.is_ok(), "reading the directory `/test_readdir/`");

        let mut readdir = readdir.unwrap();

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("foo"), "checking entry #1");
        println!("entry 1: {:#?}", next);
        assert!(next.file_type().unwrap().is_dir(), "checking entry #1");

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("bar"), "checking entry #2");
        assert!(next.file_type().unwrap().is_dir(), "checking entry #2");

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("baz"), "checking entry #3");
        assert!(next.file_type().unwrap().is_dir(), "checking entry #3");

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("a.txt"), "checking entry #2");
        assert!(next.file_type().unwrap().is_file(), "checking entry #4");

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("b.txt"), "checking entry #2");
        assert!(next.file_type().unwrap().is_file(), "checking entry #5");

        if let Some(s) = readdir.next() {
            panic!("next: {:?}", s);
        }

        let _ = fs_extra::remove_items(&["./test_readdir"]);
    }

    /*
    #[tokio::test]
    async fn test_canonicalize() {
        let fs = gen_filesystem();

        let root_dir = env!("CARGO_MANIFEST_DIR");

        let _ = fs_extra::remove_items(&["./test_canonicalize"]);

        assert_eq!(
            fs.create_dir(Path::new("./test_canonicalize")),
            Ok(()),
            "creating `test_canonicalize`"
        );

        assert_eq!(
            fs.create_dir(Path::new("./test_canonicalize/foo")),
            Ok(()),
            "creating `foo`"
        );
        assert_eq!(
            fs.create_dir(Path::new("./test_canonicalize/foo/bar")),
            Ok(()),
            "creating `bar`"
        );
        assert_eq!(
            fs.create_dir(Path::new("./test_canonicalize/foo/bar/baz")),
            Ok(()),
            "creating `baz`",
        );
        assert_eq!(
            fs.create_dir(Path::new("./test_canonicalize/foo/bar/baz/qux")),
            Ok(()),
            "creating `qux`",
        );
        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(Path::new("./test_canonicalize/foo/bar/baz/qux/hello.txt")),
                Ok(_)
            ),
            "creating `hello.txt`",
        );

        assert_eq!(
            fs.canonicalize(Path::new("./test_canonicalize")),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize")).to_path_buf()),
            "canonicalizing `/`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("foo")),
            Err(FsError::InvalidInput),
            "canonicalizing `foo`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("./test_canonicalize/././././foo/")),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize/foo")).to_path_buf()),
            "canonicalizing `/././././foo/`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("./test_canonicalize/foo/bar//")),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize/foo/bar")).to_path_buf()),
            "canonicalizing `/foo/bar//`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("./test_canonicalize/foo/bar/../bar")),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize/foo/bar")).to_path_buf()),
            "canonicalizing `/foo/bar/../bar`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("./test_canonicalize/foo/bar/../..")),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize")).to_path_buf()),
            "canonicalizing `/foo/bar/../..`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("/foo/bar/../../..")),
            Err(FsError::InvalidInput),
            "canonicalizing `/foo/bar/../../..`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("C:/foo/")),
            Err(FsError::InvalidInput),
            "canonicalizing `C:/foo/`",
        );
        assert_eq!(
            fs.canonicalize(Path::new(
                "./test_canonicalize/foo/./../foo/bar/../../foo/bar/./baz/./../baz/qux/../../baz/./qux/hello.txt"
            )),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize/foo/bar/baz/qux/hello.txt")).to_path_buf()),
            "canonicalizing a crazily stupid path name",
        );

        let _ = fs_extra::remove_items(&["./test_canonicalize"]);
    }
    */

    #[tokio::test]
    #[ignore = "Not yet supported. See https://github.com/wasmerio/wasmer/issues/3678"]
    async fn mount_to_overlapping_directories() {
        let top_level = mem_fs::FileSystem::default();
        ops::touch(&top_level, "/file.txt").unwrap();
        let nested = mem_fs::FileSystem::default();
        ops::touch(&nested, "/another-file.txt").unwrap();

        let mut fs = UnionFileSystem::default();
        fs.mount(
            "top-level",
            "/",
            false,
            Box::new(top_level),
            Some("/top-level"),
        );
        fs.mount(
            "nested",
            "/",
            false,
            Box::new(nested),
            Some("/top-level/nested"),
        );

        assert!(ops::is_dir(&fs, "/top-level"));
        assert!(ops::is_file(&fs, "/top-level/file.txt"));
        assert!(ops::is_dir(&fs, "/top-level/nested"));
        assert!(ops::is_file(&fs, "/top-level/nested/another-file.txt"));
    }
}
