//! Another implementation of the union that uses paths,
//! its not as simple as TmpFs. not currently used but was used by
//! the previoulsy implementation of Deploy - now using TmpFs

use dashmap::DashMap;

use crate::*;

use std::{path::Path, sync::Arc};

#[derive(Debug)]
pub struct MountPoint {
    pub path: PathBuf,
    pub name: String,
    pub fs: Arc<Box<dyn FileSystem + Send + Sync>>,
}

impl MountPoint {
    pub fn fs(&self) -> &(dyn FileSystem + Send + Sync) {
        self.fs.as_ref()
    }

    pub fn mount_point_ref(&self) -> MountPointRef<'_> {
        MountPointRef {
            path: self.path.clone(),
            name: self.name.clone(),
            fs: &self.fs,
        }
    }
}

/// Allows different filesystems of different types
/// to be mounted at various mount points
#[derive(Debug, Default)]
pub struct UnionFileSystem {
    pub mounts: DashMap<PathBuf, MountPoint>,
}

impl UnionFileSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.mounts.clear();
    }

    fn is_root(&self) -> bool {
        self.mounts.len() == 1 && self.mounts.contains_key(&PathBuf::from("/"))
    }

    fn prepare_path(&self, path: &Path) -> PathBuf {
        if self.is_root() {
            path.to_owned()
        } else {
            path.strip_prefix(PathBuf::from("/"))
                .unwrap_or(path)
                .to_owned()
        }
    }
}

impl UnionFileSystem {
    #[allow(clippy::type_complexity)]
    fn find_mount(
        &self,
        path: PathBuf,
    ) -> Option<(PathBuf, PathBuf, Arc<Box<dyn FileSystem + Send + Sync>>)> {
        let mut components = path.components().collect::<Vec<_>>();

        if let Some(c) = components.first().copied() {
            components.remove(0);

            let sub_path = components.into_iter().collect::<PathBuf>();

            if let Some(mount) = self.mounts.get(&PathBuf::from(c.as_os_str())) {
                return Some((
                    PathBuf::from(c.as_os_str()),
                    PathBuf::from("/").join(sub_path),
                    mount.fs.clone(),
                ));
            }
        }

        None
    }
}

impl FileSystem for UnionFileSystem {
    fn readlink(&self, path: &Path) -> Result<PathBuf> {
        let path = self.prepare_path(path);

        if path.as_os_str().is_empty() {
            Err(FsError::NotAFile)
        } else if let Some((_, path, fs)) = self.find_mount(path.to_owned()) {
            fs.readlink(&path)
        } else {
            Err(FsError::EntryNotFound)
        }
    }

    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        let path = self.prepare_path(path);

        if path.as_os_str().is_empty() {
            let entries = self
                .mounts
                .iter()
                .map(|i| DirEntry {
                    path: PathBuf::from("/").join(i.key()),
                    metadata: Ok(Metadata {
                        ft: FileType::new_dir(),
                        accessed: 0,
                        created: 0,
                        modified: 0,
                        len: 0,
                    }),
                })
                .collect::<Vec<_>>();

            Ok(ReadDir::new(entries))
        } else if let Some((prefix, path, fs)) = self.find_mount(path.to_owned()) {
            let mut entries = fs.read_dir(&path)?;

            for entry in &mut entries.data {
                let path: PathBuf = entry.path.components().skip(1).collect();
                entry.path = PathBuf::from("/").join(PathBuf::from(&prefix).join(path));
            }

            Ok(entries)
        } else {
            Err(FsError::EntryNotFound)
        }
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        let path = self.prepare_path(path);

        if path.as_os_str().is_empty() {
            Ok(())
        } else if let Some((_, path, fs)) = self.find_mount(path.to_owned()) {
            let result = fs.create_dir(&path);

            if let Err(e) = result {
                if e == FsError::AlreadyExists {
                    return Ok(());
                }
            }

            result
        } else {
            Err(FsError::EntryNotFound)
        }
    }
    fn remove_dir(&self, path: &Path) -> Result<()> {
        let path = self.prepare_path(path);

        if path.as_os_str().is_empty() {
            Err(FsError::PermissionDenied)
        } else if let Some((_, path, fs)) = self.find_mount(path.to_owned()) {
            fs.remove_dir(&path)
        } else {
            Err(FsError::EntryNotFound)
        }
    }
    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let from = self.prepare_path(from);
            let to = self.prepare_path(to);

            if from.as_os_str().is_empty() {
                Err(FsError::PermissionDenied)
            } else if let Some((prefix, path, fs)) = self.find_mount(from.to_owned()) {
                let to = to.strip_prefix(prefix).map_err(|_| FsError::InvalidInput)?;

                let to = PathBuf::from("/").join(to);

                fs.rename(&path, &to).await
            } else {
                Err(FsError::EntryNotFound)
            }
        })
    }
    fn metadata(&self, path: &Path) -> Result<Metadata> {
        let path = self.prepare_path(path);

        if path.as_os_str().is_empty() {
            Ok(Metadata {
                ft: FileType::new_dir(),
                accessed: 0,
                created: 0,
                modified: 0,
                len: 0,
            })
        } else if let Some((_, path, fs)) = self.find_mount(path.to_owned()) {
            fs.metadata(&path)
        } else {
            Err(FsError::EntryNotFound)
        }
    }
    fn symlink_metadata(&self, path: &Path) -> Result<Metadata> {
        let path = self.prepare_path(path);

        if path.as_os_str().is_empty() {
            Ok(Metadata {
                ft: FileType::new_dir(),
                accessed: 0,
                created: 0,
                modified: 0,
                len: 0,
            })
        } else if let Some((_, path, fs)) = self.find_mount(path.to_owned()) {
            fs.symlink_metadata(&path)
        } else {
            Err(FsError::EntryNotFound)
        }
    }
    fn remove_file(&self, path: &Path) -> Result<()> {
        let path = self.prepare_path(path);

        if path.as_os_str().is_empty() {
            Err(FsError::NotAFile)
        } else if let Some((_, path, fs)) = self.find_mount(path.to_owned()) {
            fs.remove_file(&path)
        } else {
            Err(FsError::EntryNotFound)
        }
    }
    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(self)
    }

    fn mount(
        &self,
        name: String,
        path: &Path,
        fs: Box<dyn FileSystem + Send + Sync>,
    ) -> Result<()> {
        let mut components = path.components().collect::<Vec<_>>();
        if let Some(c) = components.first().copied() {
            components.remove(0);

            let sub_path = components.into_iter().collect::<PathBuf>();

            if let Some(mount) = self.mounts.get(&PathBuf::from(c.as_os_str())) {
                return mount.fs.mount(name, sub_path.as_path(), fs);
            }

            let fs = if sub_path.components().next().is_none() {
                fs
            } else {
                let union = UnionFileSystem::new();
                union.mount(name.clone(), sub_path.as_path(), fs)?;

                Box::new(union)
            };

            let fs = Arc::new(fs);

            let mount = MountPoint {
                path: PathBuf::from(c.as_os_str()),
                name,
                fs,
            };

            self.mounts.insert(PathBuf::from(c.as_os_str()), mount);
        } else {
            return Err(FsError::EntryNotFound);
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct MountPointRef<'a> {
    pub path: PathBuf,
    pub name: String,
    pub fs: &'a dyn FileSystem,
}

impl FileOpener for UnionFileSystem {
    fn open(
        &self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync>> {
        let path = self.prepare_path(path);

        if path.as_os_str().is_empty() {
            Err(FsError::NotAFile)
        } else {
            let parent = path.parent().unwrap();
            let file_name = path.file_name().unwrap();
            if let Some((_, path, fs)) = self.find_mount(parent.to_owned()) {
                fs.new_open_options()
                    .options(conf.clone())
                    .open(path.join(file_name))
            } else {
                Err(FsError::EntryNotFound)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        path::{Path, PathBuf},
    };

    use tokio::io::AsyncWriteExt;

    use crate::{mem_fs, FileSystem as FileSystemTrait, FsError, UnionFileSystem};

    use super::{FileOpener, OpenOptionsConfig};

    fn gen_filesystem() -> UnionFileSystem {
        let union = UnionFileSystem::new();
        let a = mem_fs::FileSystem::default();
        let b = mem_fs::FileSystem::default();
        let c = mem_fs::FileSystem::default();
        let d = mem_fs::FileSystem::default();
        let e = mem_fs::FileSystem::default();
        let f = mem_fs::FileSystem::default();
        let g = mem_fs::FileSystem::default();
        let h = mem_fs::FileSystem::default();

        union
            .mount(
                "mem_fs_1".to_string(),
                PathBuf::from("/test_new_filesystem").as_path(),
                Box::new(a),
            )
            .unwrap();
        union
            .mount(
                "mem_fs_2".to_string(),
                PathBuf::from("/test_create_dir").as_path(),
                Box::new(b),
            )
            .unwrap();
        union
            .mount(
                "mem_fs_3".to_string(),
                PathBuf::from("/test_remove_dir").as_path(),
                Box::new(c),
            )
            .unwrap();
        union
            .mount(
                "mem_fs_4".to_string(),
                PathBuf::from("/test_rename").as_path(),
                Box::new(d),
            )
            .unwrap();
        union
            .mount(
                "mem_fs_5".to_string(),
                PathBuf::from("/test_metadata").as_path(),
                Box::new(e),
            )
            .unwrap();
        union
            .mount(
                "mem_fs_6".to_string(),
                PathBuf::from("/test_remove_file").as_path(),
                Box::new(f),
            )
            .unwrap();
        union
            .mount(
                "mem_fs_6".to_string(),
                PathBuf::from("/test_readdir").as_path(),
                Box::new(g),
            )
            .unwrap();
        union
            .mount(
                "mem_fs_6".to_string(),
                PathBuf::from("/test_canonicalize").as_path(),
                Box::new(h),
            )
            .unwrap();

        union
    }

    fn gen_nested_filesystem() -> UnionFileSystem {
        let union = UnionFileSystem::new();
        let a = mem_fs::FileSystem::default();
        a.open(
            &PathBuf::from("/data-a.txt"),
            &OpenOptionsConfig {
                read: true,
                write: true,
                create_new: false,
                create: true,
                append: false,
                truncate: false,
            },
        )
        .unwrap();
        let b = mem_fs::FileSystem::default();
        b.open(
            &PathBuf::from("/data-b.txt"),
            &OpenOptionsConfig {
                read: true,
                write: true,
                create_new: false,
                create: true,
                append: false,
                truncate: false,
            },
        )
        .unwrap();

        union
            .mount(
                "mem_fs_1".to_string(),
                PathBuf::from("/app/a").as_path(),
                Box::new(a),
            )
            .unwrap();
        union
            .mount(
                "mem_fs_2".to_string(),
                PathBuf::from("/app/b").as_path(),
                Box::new(b),
            )
            .unwrap();

        union
    }

    #[tokio::test]
    async fn test_nested_read_dir() {
        let fs = gen_nested_filesystem();

        let root_contents: Vec<PathBuf> = fs
            .read_dir(&PathBuf::from("/"))
            .unwrap()
            .map(|e| e.unwrap().path.clone())
            .collect();
        assert_eq!(root_contents, vec![PathBuf::from("/app")]);

        let app_contents: HashSet<PathBuf> = fs
            .read_dir(&PathBuf::from("/app"))
            .unwrap()
            .map(|e| e.unwrap().path)
            .collect();
        assert_eq!(
            app_contents,
            HashSet::from_iter([PathBuf::from("/app/a"), PathBuf::from("/app/b")].into_iter())
        );

        let a_contents: Vec<PathBuf> = fs
            .read_dir(&PathBuf::from("/app/a"))
            .unwrap()
            .map(|e| e.unwrap().path.clone())
            .collect();
        assert_eq!(a_contents, vec![PathBuf::from("/app/a/data-a.txt")]);

        let b_contents: Vec<PathBuf> = fs
            .read_dir(&PathBuf::from("/app/b"))
            .unwrap()
            .map(|e| e.unwrap().path)
            .collect();
        assert_eq!(b_contents, vec![PathBuf::from("/app/b/data-b.txt")]);
    }

    #[tokio::test]
    async fn test_nested_metadata() {
        let fs = gen_nested_filesystem();

        assert!(fs.metadata(&PathBuf::from("/")).is_ok());
        assert!(fs.metadata(&PathBuf::from("/app")).is_ok());
        assert!(fs.metadata(&PathBuf::from("/app/a")).is_ok());
        assert!(fs.metadata(&PathBuf::from("/app/b")).is_ok());
        assert!(fs.metadata(&PathBuf::from("/app/a/data-a.txt")).is_ok());
        assert!(fs.metadata(&PathBuf::from("/app/b/data-b.txt")).is_ok());
    }

    #[tokio::test]
    async fn test_nested_symlink_metadata() {
        let fs = gen_nested_filesystem();

        assert!(fs.symlink_metadata(&PathBuf::from("/")).is_ok());
        assert!(fs.symlink_metadata(&PathBuf::from("/app")).is_ok());
        assert!(fs.symlink_metadata(&PathBuf::from("/app/a")).is_ok());
        assert!(fs.symlink_metadata(&PathBuf::from("/app/b")).is_ok());
        assert!(fs
            .symlink_metadata(&PathBuf::from("/app/a/data-a.txt"))
            .is_ok());
        assert!(fs
            .symlink_metadata(&PathBuf::from("/app/b/data-b.txt"))
            .is_ok());
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

        assert_eq!(fs.create_dir(Path::new("/")), Ok(()));

        assert_eq!(fs.create_dir(Path::new("/test_create_dir")), Ok(()));

        assert_eq!(
            fs.create_dir(Path::new("/test_create_dir/foo")),
            Ok(()),
            "creating a directory",
        );

        let cur_dir = read_dir_names(&fs, "/test_create_dir");

        if !cur_dir.contains(&"foo".to_string()) {
            panic!("cur_dir does not contain foo: {cur_dir:#?}");
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

        assert_eq!(
            fs.remove_dir(Path::new("/")),
            Err(FsError::PermissionDenied),
            "cannot remove the root directory",
        );

        assert_eq!(
            fs.remove_dir(Path::new("/foo")),
            Err(FsError::EntryNotFound),
            "cannot remove a directory that doesn't exist",
        );

        assert_eq!(fs.create_dir(Path::new("/test_remove_dir")), Ok(()));

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

        assert_eq!(
            fs.rename(Path::new("/"), Path::new("/bar")).await,
            Err(FsError::PermissionDenied),
            "renaming a directory that has no parent",
        );
        assert_eq!(
            fs.rename(Path::new("/foo"), Path::new("/")).await,
            Err(FsError::EntryNotFound),
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
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(Path::new("/test_rename/bar/hello1.txt"))
                .is_ok(),
            "creating a new file (`hello1.txt`)",
        );
        assert!(
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(Path::new("/test_rename/bar/hello2.txt"))
                .is_ok(),
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
            println!("qux does not exist: {bar_dir:?}")
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

        let root_metadata = fs.metadata(Path::new("/test_metadata")).unwrap();

        assert!(root_metadata.ft.dir);
        assert_eq!(root_metadata.accessed, root_metadata.created);
        assert_eq!(root_metadata.modified, root_metadata.created);
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

        assert!(
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(Path::new("/test_remove_file/foo.txt"))
                .is_ok(),
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
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(Path::new("/test_readdir/a.txt"))
                .is_ok(),
            "creating `a.txt`",
        );
        assert!(
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(Path::new("/test_readdir/b.txt"))
                .is_ok(),
            "creating `b.txt`",
        );

        println!("fs: {fs:?}");

        let readdir = fs.read_dir(Path::new("/test_readdir"));

        assert!(readdir.is_ok(), "reading the directory `/test_readdir/`");

        let mut readdir = readdir.unwrap();

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("foo"), "checking entry #1");
        println!("entry 1: {next:#?}");
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
            panic!("next: {s:?}");
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
}
