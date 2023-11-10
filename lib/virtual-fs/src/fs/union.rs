//! Another implementation of the union that uses paths,
//! its not as simple as TmpFs. not currently used but was used by
//! the previoulsy implementation of Deploy - now using TmpFs

use crate::ops::PathClean;
use crate::*;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tracing::debug;

/// Allows different filesystems of different types
/// to be mounted at various mount points
#[derive(Debug, Default)]
pub struct UnionFileSystem {
    pub dirs: HashMap<String, Box<dyn FileSystem + Send + Sync + 'static>>,
}

impl UnionFileSystem {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn mount(
        &mut self,
        path: &PathBuf,
        fs: Box<dyn FileSystem + Send + Sync + 'static>,
    ) -> Result<()> {
        let path = path.clean_safely()?;
        match self.get_dir_for_path_mut(path.clone()) {
            Ok((dir, mounted_fs)) => {
                if let Some(union_fs) = mounted_fs
                    .upcast_any_mut()
                    .downcast_mut::<UnionFileSystem>()
                {
                    return union_fs.mount(&dir, fs);
                }
                Err(FsError::AlreadyExists)
            }
            Err(FsError::EntryNotFound) | Err(_) => {
                let mut path_iter = path.iter();
                let mut new_fs = fs;
                let base_dir = path_iter.next().unwrap();
                for dir in path_iter.rev() {
                    let mut fs = UnionFileSystem::new();
                    fs.mount(&dir.to_owned().into(), new_fs)?;
                    new_fs = Box::new(fs);
                }
                self.dirs
                    .insert(base_dir.to_str().unwrap().to_string(), new_fs);
                Ok(())
            }
        }
    }
    fn get_dir_for_path(
        &self,
        path: &PathBuf,
    ) -> Result<(PathBuf, &(dyn FileSystem + Send + Sync + 'static))> {
        let mut path_iter = path.iter();
        let base_dir = path_iter.next().unwrap();
        self.dirs
            .get(base_dir.to_str().unwrap())
            .ok_or(FsError::EntryNotFound)
            .map(|fs| {
                let path = std::iter::once(std::ffi::OsStr::new("/"))
                    .chain(path_iter)
                    .collect();
                // if path == Path::new("") {
                //     return (PathBuf::from("/"), fs.as_ref());
                // }
                // else {}
                (path, fs.as_ref())
            })
    }
    fn get_dir_for_path_mut(
        &mut self,
        path: PathBuf,
    ) -> Result<(PathBuf, &mut (dyn FileSystem + Send + Sync + 'static))> {
        let mut path_iter = path.iter();
        let base_dir = path_iter.next().unwrap();
        self.dirs
            .get_mut(base_dir.to_str().unwrap())
            .ok_or(FsError::EntryNotFound)
            .map(|fs| (path_iter.collect(), fs.as_mut()))
    }
}

impl FileSystem for UnionFileSystem {
    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        println!("read_dir: path={}", path.display());
        let (dir, fs) = self.get_dir_for_path(&path.clean_safely()?)?;
        println!("READDIR {}", dir.display());
        fs.read_dir(&dir)
    }
    fn create_dir(&self, path: &Path) -> Result<()> {
        debug!("create_dir: path={}", path.display());
        if path == Path::new("/") {
            return Err(FsError::BaseNotDirectory);
        }
        let (dir, fs) = self
            .get_dir_for_path(&path.clean_safely()?)
            .map_err(|_e| FsError::PermissionDenied)?;
        fs.create_dir(&dir)
    }
    fn remove_dir(&self, path: &Path) -> Result<()> {
        debug!("remove_dir: path={}", path.display());
        let cleaned_dir = path.clean_safely()?;
        let (dir, fs) = self
            .get_dir_for_path(&cleaned_dir)
            .map_err(|_e| FsError::PermissionDenied)?;
        // If we delete the dir, we just unmount it
        // self.dirs.remove(cleaned_dir.to_str().unwrap());
        if dir == Path::new("/") {
            return Ok(());
        }
        fs.remove_dir(&dir)
    }
    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async {
            println!("rename: from={} to={}", from.display(), to.display());
            let from = from.clean_safely()?;
            let to = to.clean_safely()?;
            if from.parent().is_none() {
                return Err(FsError::BaseNotDirectory);
            }
            if to.parent().is_none() {
                return Err(FsError::BaseNotDirectory);
            }
            let (from_dir, from_fs) = self
                .get_dir_for_path(&from)
                .map_err(|_e| FsError::PermissionDenied)?;
            let (to_dir, to_fs) = self
                .get_dir_for_path(&to)
                .map_err(|_e| FsError::PermissionDenied)?;
            // We indirectly check that the from_dir path exists by calling metadata
            let _from_metadata = from_fs.metadata(&from_dir)?;
            let to_metadata = from_fs.metadata(&from_dir);
            if to_metadata.is_ok() {
                // The folder that we want to rename to already exists
                return Err(FsError::AlreadyExists);
            }

            ops::create_dir_all(to_fs, to_dir.parent().ok_or(FsError::BaseNotDirectory)?)?;
            if from_dir.is_dir() {
                unimplemented!("rename dir not yet implemented in UnionFilesystem");
            } else {
                ops::copy_reference_ext(from_fs, to_fs, &from_dir, &to_dir).await?;
                from_fs.remove_file(&from_dir)
            }
        })
    }
    fn metadata(&self, path: &Path) -> Result<Metadata> {
        println!("metadata: path={}", path.display());
        let path = path.clean_safely()?;
        if path == Path::new(".") {
            return Ok(Metadata {
                ft: FileType::new_dir(),
                accessed: 0,
                created: 0,
                modified: 0,
                len: 0,
            });
        }
        let (dir, fs) = self
            .get_dir_for_path(&path)
            .map_err(|_e| FsError::PermissionDenied)?;
        fs.metadata(&dir)
    }
    fn symlink_metadata(&self, path: &Path) -> Result<Metadata> {
        debug!("symlink_metadata: path={}", path.display());
        let path = path.clean_safely()?;
        if path == Path::new(".") {
            return Ok(Metadata {
                ft: FileType::new_dir(),
                accessed: 0,
                created: 0,
                modified: 0,
                len: 0,
            });
        }
        let (dir, fs) = self
            .get_dir_for_path(&path.clean_safely()?)
            .map_err(|_e| FsError::PermissionDenied)?;
        fs.symlink_metadata(&dir)
    }
    fn remove_file(&self, path: &Path) -> Result<()> {
        println!("remove_file: path={}", path.display());
        let (dir, fs) = self
            .get_dir_for_path(&path.clean_safely()?)
            .map_err(|_e| FsError::BaseNotDirectory)?;
        fs.remove_file(&dir)
    }
    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(self)
    }
}

impl FileOpener for UnionFileSystem {
    fn open(
        &self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync>> {
        debug!("open: path={}", path.display());
        let path = path.clean_safely()?;
        if path == Path::new(".") {
            return Err(FsError::BaseNotDirectory);
        }
        let (dir, fs) = self
            .get_dir_for_path(&path)
            .map_err(|_e| FsError::BaseNotDirectory)?;
        let mut open_options = fs.new_open_options();
        open_options.options(conf.clone()).open(&dir)
    }
}

#[cfg(test)]
mod tests {

    use super::UnionFileSystem;
    use std::path::{Path, PathBuf};
    use tokio::io::AsyncWriteExt;

    use crate::{mem_fs, ops, FileSystem as FileSystemTrait, FsError, VirtualFile};

    #[ignore]
    #[test]
    fn test_mount_new_filesystem() {
        let mut ufs = UnionFileSystem::new();
        assert!(ufs.metadata(&Path::new(".")).unwrap().is_dir());

        // Attempt to mount the MemoryFS at the specified path
        let mem_fs = mem_fs::FileSystem::default(); // Assuming MemoryFS is another FileSystem that can be mounted
        let mount_path = PathBuf::from("/mnt");
        let result = ufs.mount(&mount_path, Box::new(mem_fs));

        // Assert that the mount was successful
        assert!(result.is_ok());
        assert!(ufs.metadata(&Path::new("/mnt")).unwrap().is_dir());
        assert_eq!(
            ufs.read_dir(&Path::new("/mnt"))
                .unwrap()
                .collect::<Vec<_>>(),
            vec![]
        );
    }

    #[test]
    fn test_mounting_fs_on_existing_path() {
        let mut ufs = UnionFileSystem::new();
        let mount_path = PathBuf::from("/mnt");
        let fs1 = Box::new(mem_fs::FileSystem::default());
        let fs2 = Box::new(mem_fs::FileSystem::default());

        ufs.mount(&mount_path, fs1).unwrap();
        let result = ufs.mount(&mount_path, fs2);
        assert!(matches!(result, Err(FsError::AlreadyExists)));
    }

    #[ignore]
    #[test]
    fn test_mounting_fs_on_nested_path() {
        let mut ufs = UnionFileSystem::new();
        let mount_path1 = PathBuf::from("/mnt/subdir");
        let fs1 = Box::new(mem_fs::FileSystem::default());

        let result: Result<(), FsError> = ufs.mount(&mount_path1, fs1);
        // Depending on the implementation details, this might be OK or an error.
        // If it's designed to allow nested mounts, this should be OK.
        assert!(result.is_ok());
        assert!(ufs.dirs.contains_key("mnt"));

        assert!(ufs.metadata(&Path::new("mnt")).unwrap().is_dir());
        assert!(ufs.metadata(&Path::new("mnt/subdir")).unwrap().is_dir());

        assert!(ufs.metadata(&Path::new("mnt/nonexistent")).is_err());
        assert!(ufs.metadata(&Path::new("mnt/subdir/nonexistent")).is_err());
    }

    #[test]
    fn test_mounting_fs_on_colliding_path() {
        let mut ufs = UnionFileSystem::new();
        let mount_path1 = PathBuf::from("/mnt");
        let mount_path2 = PathBuf::from("/mnt/subdir");
        let fs1 = Box::new(mem_fs::FileSystem::default());
        let fs2 = Box::new(mem_fs::FileSystem::default());

        ufs.mount(&mount_path1, fs1).unwrap();
        let result = ufs.mount(&mount_path2, fs2);
        assert_eq!(result, Err(FsError::AlreadyExists));
    }
    #[test]
    fn test_mounting_fs_on_non_colliding_paths() {
        let mut ufs = UnionFileSystem::new();
        let mount_path1 = PathBuf::from("/mnt/subdir1");
        let mount_path2 = PathBuf::from("/mnt/subdir2");
        let fs1 = Box::new(mem_fs::FileSystem::default());
        let fs2 = Box::new(mem_fs::FileSystem::default());

        ufs.mount(&mount_path1, fs1).unwrap();
        let result = ufs.mount(&mount_path2, fs2);
        assert!(result.is_ok());
    }

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

        union
            .mount(&PathBuf::from("/test_new_filesystem"), Box::new(a))
            .unwrap();
        union
            .mount(&PathBuf::from("/test_create_dir"), Box::new(b))
            .unwrap();
        union
            .mount(&PathBuf::from("/test_remove_dir"), Box::new(c))
            .unwrap();
        union
            .mount(&PathBuf::from("/test_rename"), Box::new(d))
            .unwrap();
        union
            .mount(&PathBuf::from("/test_metadata"), Box::new(e))
            .unwrap();
        union
            .mount(&PathBuf::from("/test_remove_file"), Box::new(f))
            .unwrap();
        union
            .mount(&PathBuf::from("/test_readdir"), Box::new(g))
            .unwrap();
        union
            .mount(&PathBuf::from("/test_canonicalize"), Box::new(h))
            .unwrap();

        union
    }

    #[tokio::test]
    async fn test_new_filesystem() {
        let fs = gen_filesystem();
        assert!(
            fs.read_dir(Path::new("/test_new_filesystem")).is_ok(),
            "unionfs can read root"
        );
        let mut file_write: Box<dyn VirtualFile + Send + Sync> = fs
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

    // #[tokio::test]
    // async fn test_remove_dir() {
    //     let fs = gen_filesystem();

    //     let _ = fs_extra::remove_items(&["/test_remove_dir"]);

    //     assert_eq!(
    //         fs.remove_dir(Path::new("/")),
    //         Err(FsError::BaseNotDirectory),
    //         "removing a directory that has no parent",
    //     );

    //     assert_eq!(
    //         fs.remove_dir(Path::new("/foo")),
    //         Err(FsError::EntryNotFound),
    //         "cannot remove a directory that doesn't exist",
    //     );

    //     assert_eq!(
    //         fs.create_dir(Path::new("/test_remove_dir")),
    //         Ok(()),
    //         "creating a directory",
    //     );

    //     assert_eq!(
    //         fs.create_dir(Path::new("/test_remove_dir/foo")),
    //         Ok(()),
    //         "creating a directory",
    //     );

    //     assert_eq!(
    //         fs.create_dir(Path::new("/test_remove_dir/foo/bar")),
    //         Ok(()),
    //         "creating a sub-directory",
    //     );

    //     assert!(
    //         read_dir_names(&fs, "/test_remove_dir/foo").contains(&"bar".to_string()),
    //         "./foo/bar exists"
    //     );

    //     assert_eq!(
    //         fs.remove_dir(Path::new("/test_remove_dir/foo")),
    //         Err(FsError::DirectoryNotEmpty),
    //         "removing a directory that has children",
    //     );

    //     assert_eq!(
    //         fs.remove_dir(Path::new("/test_remove_dir/foo/bar")),
    //         Ok(()),
    //         "removing a sub-directory",
    //     );

    //     assert_eq!(
    //         fs.remove_dir(Path::new("/test_remove_dir/foo")),
    //         Ok(()),
    //         "removing a directory",
    //     );

    //     assert!(
    //         !read_dir_names(&fs, "/test_remove_dir").contains(&"foo".to_string()),
    //         "the foo directory still exists"
    //     );

    //     let _ = fs_extra::remove_items(&["/test_remove_dir"]);
    // }

    fn read_dir_names(fs: &dyn crate::FileSystem, path: &str) -> Vec<String> {
        fs.read_dir(Path::new(path))
            .unwrap()
            .filter_map(|entry| Some(entry.ok()?.file_name().to_str()?.to_string()))
            .collect::<Vec<_>>()
    }

    // #[tokio::test]
    // async fn test_rename() {
    //     let fs = gen_filesystem();

    //     let _ = fs_extra::remove_items(&["./test_rename"]);

    //     assert_eq!(
    //         fs.rename(Path::new("/"), Path::new("/bar")).await,
    //         Err(FsError::BaseNotDirectory),
    //         "renaming a directory that has no parent",
    //     );
    //     assert_eq!(
    //         fs.rename(Path::new("/foo"), Path::new("/")).await,
    //         Err(FsError::BaseNotDirectory),
    //         "renaming to a directory that has no parent",
    //     );

    //     assert_eq!(fs.create_dir(Path::new("/test_rename")), Ok(()));
    //     assert_eq!(fs.create_dir(Path::new("/test_rename/foo")), Ok(()));
    //     assert_eq!(fs.create_dir(Path::new("/test_rename/foo/qux")), Ok(()));

    //     assert_eq!(
    //         fs.rename(
    //             Path::new("/test_rename/foo"),
    //             Path::new("/test_rename/bar/baz")
    //         )
    //         .await,
    //         Err(FsError::EntryNotFound),
    //         "renaming to a directory that has parent that doesn't exist",
    //     );

    //     assert_eq!(fs.create_dir(Path::new("/test_rename/bar")), Ok(()));

    //     assert_eq!(
    //         fs.rename(Path::new("/test_rename/foo"), Path::new("/test_rename/bar"))
    //             .await,
    //         Ok(()),
    //         "renaming to a directory that has parent that exists",
    //     );

    //     assert!(
    //         matches!(
    //             fs.new_open_options()
    //                 .write(true)
    //                 .create_new(true)
    //                 .open(Path::new("/test_rename/bar/hello1.txt")),
    //             Ok(_),
    //         ),
    //         "creating a new file (`hello1.txt`)",
    //     );
    //     assert!(
    //         matches!(
    //             fs.new_open_options()
    //                 .write(true)
    //                 .create_new(true)
    //                 .open(Path::new("/test_rename/bar/hello2.txt")),
    //             Ok(_),
    //         ),
    //         "creating a new file (`hello2.txt`)",
    //     );

    //     let cur_dir = read_dir_names(&fs, "/test_rename");

    //     assert!(
    //         !cur_dir.contains(&"foo".to_string()),
    //         "the foo directory still exists"
    //     );

    //     assert!(
    //         cur_dir.contains(&"bar".to_string()),
    //         "the bar directory still exists"
    //     );

    //     let bar_dir = read_dir_names(&fs, "/test_rename/bar");

    //     if !bar_dir.contains(&"qux".to_string()) {
    //         println!("qux does not exist: {:?}", bar_dir)
    //     }

    //     let qux_dir = read_dir_names(&fs, "/test_rename/bar/qux");

    //     assert!(qux_dir.is_empty(), "the qux directory is empty");

    //     assert!(
    //         read_dir_names(&fs, "/test_rename/bar").contains(&"hello1.txt".to_string()),
    //         "the /bar/hello1.txt file exists"
    //     );

    //     assert!(
    //         read_dir_names(&fs, "/test_rename/bar").contains(&"hello2.txt".to_string()),
    //         "the /bar/hello2.txt file exists"
    //     );

    //     assert_eq!(
    //         fs.create_dir(Path::new("/test_rename/foo")),
    //         Ok(()),
    //         "create ./foo again",
    //     );

    //     assert_eq!(
    //         fs.rename(
    //             Path::new("/test_rename/bar/hello2.txt"),
    //             Path::new("/test_rename/foo/world2.txt")
    //         )
    //         .await,
    //         Ok(()),
    //         "renaming (and moving) a file",
    //     );

    //     assert_eq!(
    //         fs.rename(
    //             Path::new("/test_rename/foo"),
    //             Path::new("/test_rename/bar/baz")
    //         )
    //         .await,
    //         Ok(()),
    //         "renaming a directory",
    //     );

    //     assert_eq!(
    //         fs.rename(
    //             Path::new("/test_rename/bar/hello1.txt"),
    //             Path::new("/test_rename/bar/world1.txt")
    //         )
    //         .await,
    //         Ok(()),
    //         "renaming a file (in the same directory)",
    //     );

    //     assert!(
    //         read_dir_names(&fs, "/test_rename").contains(&"bar".to_string()),
    //         "./bar exists"
    //     );

    //     assert!(
    //         read_dir_names(&fs, "/test_rename/bar").contains(&"baz".to_string()),
    //         "/bar/baz exists"
    //     );
    //     assert!(
    //         !read_dir_names(&fs, "/test_rename").contains(&"foo".to_string()),
    //         "foo does not exist anymore"
    //     );
    //     assert!(
    //         read_dir_names(&fs, "/test_rename/bar/baz").contains(&"world2.txt".to_string()),
    //         "/bar/baz/world2.txt exists"
    //     );
    //     assert!(
    //         read_dir_names(&fs, "/test_rename/bar").contains(&"world1.txt".to_string()),
    //         "/bar/world1.txt (ex hello1.txt) exists"
    //     );
    //     assert!(
    //         !read_dir_names(&fs, "/test_rename/bar").contains(&"hello1.txt".to_string()),
    //         "hello1.txt was moved"
    //     );
    //     assert!(
    //         !read_dir_names(&fs, "/test_rename/bar").contains(&"hello2.txt".to_string()),
    //         "hello2.txt was moved"
    //     );
    //     assert!(
    //         read_dir_names(&fs, "/test_rename/bar/baz").contains(&"world2.txt".to_string()),
    //         "world2.txt was moved to the correct place"
    //     );

    //     let _ = fs_extra::remove_items(&["/test_rename"]);
    // }

    // #[tokio::test]
    // async fn test_metadata() {
    //     use std::thread::sleep;
    //     use std::time::Duration;

    //     let fs = gen_filesystem();

    //     let _ = fs_extra::remove_items(&["/test_metadata"]);

    //     assert_eq!(fs.create_dir(Path::new("/test_metadata")), Ok(()));

    //     let root_metadata = fs.metadata(Path::new("/test_metadata")).unwrap();

    //     assert!(root_metadata.ft.dir);
    //     assert!(root_metadata.accessed == root_metadata.created);
    //     assert!(root_metadata.modified == root_metadata.created);
    //     assert!(root_metadata.modified > 0);

    //     assert_eq!(fs.create_dir(Path::new("/test_metadata/foo")), Ok(()));

    //     let foo_metadata = fs.metadata(Path::new("/test_metadata/foo"));
    //     assert!(foo_metadata.is_ok());
    //     let foo_metadata = foo_metadata.unwrap();

    //     assert!(foo_metadata.ft.dir);
    //     assert!(foo_metadata.accessed == foo_metadata.created);
    //     assert!(foo_metadata.modified == foo_metadata.created);
    //     assert!(foo_metadata.modified > 0);

    //     sleep(Duration::from_secs(3));

    //     assert_eq!(
    //         fs.rename(
    //             Path::new("/test_metadata/foo"),
    //             Path::new("/test_metadata/bar")
    //         )
    //         .await,
    //         Ok(())
    //     );

    //     let bar_metadata = fs.metadata(Path::new("/test_metadata/bar")).unwrap();
    //     assert!(bar_metadata.ft.dir);
    //     assert!(bar_metadata.accessed == foo_metadata.accessed);
    //     assert!(bar_metadata.created == foo_metadata.created);
    //     assert!(bar_metadata.modified > foo_metadata.modified);

    //     let root_metadata = fs.metadata(Path::new("/test_metadata/bar")).unwrap();
    //     assert!(
    //         root_metadata.modified > foo_metadata.modified,
    //         "the parent modified time was updated"
    //     );

    //     let _ = fs_extra::remove_items(&["/test_metadata"]);
    // }

    // #[tokio::test]
    // async fn test_remove_file() {
    //     let fs = gen_filesystem();

    //     let _ = fs_extra::remove_items(&["./test_remove_file"]);

    //     assert!(fs.create_dir(Path::new("/test_remove_file")).is_ok());

    //     assert!(
    //         matches!(
    //             fs.new_open_options()
    //                 .write(true)
    //                 .create_new(true)
    //                 .open(Path::new("/test_remove_file/foo.txt")),
    //             Ok(_)
    //         ),
    //         "creating a new file",
    //     );

    //     assert!(read_dir_names(&fs, "/test_remove_file").contains(&"foo.txt".to_string()));

    //     assert_eq!(
    //         fs.remove_file(Path::new("/test_remove_file/foo.txt")),
    //         Ok(()),
    //         "removing a file that exists",
    //     );

    //     assert!(!read_dir_names(&fs, "/test_remove_file").contains(&"foo.txt".to_string()));

    //     assert_eq!(
    //         fs.remove_file(Path::new("/test_remove_file/foo.txt")),
    //         Err(FsError::EntryNotFound),
    //         "removing a file that doesn't exists",
    //     );

    //     let _ = fs_extra::remove_items(&["./test_remove_file"]);
    // }

    // #[tokio::test]
    // async fn test_readdir() {
    //     let fs = gen_filesystem();

    //     assert_eq!(
    //         fs.create_dir(Path::new("/test_readdir/foo")),
    //         Ok(()),
    //         "creating `foo`"
    //     );
    //     assert_eq!(
    //         fs.create_dir(Path::new("/test_readdir/foo/sub")),
    //         Ok(()),
    //         "creating `sub`"
    //     );
    //     assert_eq!(
    //         fs.create_dir(Path::new("/test_readdir/bar")),
    //         Ok(()),
    //         "creating `bar`"
    //     );
    //     assert_eq!(
    //         fs.create_dir(Path::new("/test_readdir/baz")),
    //         Ok(()),
    //         "creating `bar`"
    //     );
    //     assert!(
    //         matches!(
    //             fs.new_open_options()
    //                 .write(true)
    //                 .create_new(true)
    //                 .open(Path::new("/test_readdir/a.txt")),
    //             Ok(_)
    //         ),
    //         "creating `a.txt`",
    //     );
    //     assert!(
    //         matches!(
    //             fs.new_open_options()
    //                 .write(true)
    //                 .create_new(true)
    //                 .open(Path::new("/test_readdir/b.txt")),
    //             Ok(_)
    //         ),
    //         "creating `b.txt`",
    //     );

    //     println!("fs: {:?}", fs);

    //     let readdir = fs.read_dir(Path::new("/test_readdir"));

    //     assert!(readdir.is_ok(), "reading the directory `/test_readdir/`");

    //     let mut readdir = readdir.unwrap();

    //     let next = readdir.next().unwrap().unwrap();
    //     assert!(next.path.ends_with("foo"), "checking entry #1");
    //     println!("entry 1: {:#?}", next);
    //     assert!(next.file_type().unwrap().is_dir(), "checking entry #1");

    //     let next = readdir.next().unwrap().unwrap();
    //     assert!(next.path.ends_with("bar"), "checking entry #2");
    //     assert!(next.file_type().unwrap().is_dir(), "checking entry #2");

    //     let next = readdir.next().unwrap().unwrap();
    //     assert!(next.path.ends_with("baz"), "checking entry #3");
    //     assert!(next.file_type().unwrap().is_dir(), "checking entry #3");

    //     let next = readdir.next().unwrap().unwrap();
    //     assert!(next.path.ends_with("a.txt"), "checking entry #2");
    //     assert!(next.file_type().unwrap().is_file(), "checking entry #4");

    //     let next = readdir.next().unwrap().unwrap();
    //     assert!(next.path.ends_with("b.txt"), "checking entry #2");
    //     assert!(next.file_type().unwrap().is_file(), "checking entry #5");

    //     if let Some(s) = readdir.next() {
    //         panic!("next: {:?}", s);
    //     }

    //     let _ = fs_extra::remove_items(&["./test_readdir"]);
    // }

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

    // #[tokio::test]
    // #[ignore = "Not yet supported. See https://github.com/wasmerio/wasmer/issues/3678"]
    // async fn mount_to_overlapping_directories() {
    //     let top_level = mem_fs::FileSystem::default();
    //     ops::touch(&top_level, "/file.txt").unwrap();
    //     let nested = mem_fs::FileSystem::default();
    //     ops::touch(&nested, "/another-file.txt").unwrap();

    //     let mut fs = UnionFileSystem::default();
    //     fs.mount(
    //         "top-level",
    //         "/",
    //         false,
    //         Box::new(top_level),
    //         Some("/top-level"),
    //     );
    //     fs.mount(
    //         "nested",
    //         "/",
    //         false,
    //         Box::new(nested),
    //         Some("/top-level/nested"),
    //     );

    //     assert!(ops::is_dir(&fs, "/top-level"));
    //     assert!(ops::is_file(&fs, "/top-level/file.txt"));
    //     assert!(ops::is_dir(&fs, "/top-level/nested"));
    //     assert!(ops::is_file(&fs, "/top-level/nested/another-file.txt"));
    // }
}
