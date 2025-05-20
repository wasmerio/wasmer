use super::filesystem::InodeResolution;
use super::*;
use crate::{FileType, FsError, Metadata, OpenOptionsConfig, Result, VirtualFile};
use shared_buffer::OwnedBuffer;
use std::path::Path;
use tracing::*;

impl FileSystem {
    /// Inserts a readonly file into the file system that uses copy-on-write
    /// (this is required for zero-copy creation of the same file)
    pub fn insert_ro_file(&self, path: &Path, contents: OwnedBuffer) -> Result<()> {
        let _ = crate::FileSystem::remove_file(self, path);
        let (inode_of_parent, maybe_inode_of_file, name_of_file) = self.insert_inode(path)?;

        let inode_of_parent = match inode_of_parent {
            InodeResolution::Found(a) => a,
            InodeResolution::Redirect(..) => {
                return Err(FsError::InvalidInput);
            }
        };

        match maybe_inode_of_file {
            // The file already exists, then it can not be inserted.
            Some(_inode_of_file) => return Err(FsError::AlreadyExists),

            // The file doesn't already exist; it's OK to create it if
            None => {
                // Write lock.
                let mut fs = self.inner.write().map_err(|_| FsError::Lock)?;

                let file = ReadOnlyFile::new(contents);
                let file_len = file.len() as u64;

                // Creating the file in the storage.
                let inode_of_file = fs.storage.vacant_entry().key();
                let real_inode_of_file = fs.storage.insert(Node::ReadOnlyFile(ReadOnlyFileNode {
                    inode: inode_of_file,
                    name: name_of_file,
                    file,
                    metadata: {
                        let time = time();

                        Metadata {
                            ft: FileType {
                                file: true,
                                ..Default::default()
                            },
                            accessed: time,
                            created: time,
                            modified: time,
                            len: file_len,
                        }
                    },
                }));

                assert_eq!(
                    inode_of_file, real_inode_of_file,
                    "new file inode should have been correctly calculated",
                );

                // Adding the new directory to its parent.
                fs.add_child_to_node(inode_of_parent, inode_of_file)?;

                inode_of_file
            }
        };
        Ok(())
    }

    /// Inserts a arc file into the file system that references another file
    /// in another file system (does not copy the real data)
    pub fn insert_arc_file_at(
        &self,
        target_path: PathBuf,
        fs: Arc<dyn crate::FileSystem + Send + Sync>,
        source_path: PathBuf,
    ) -> Result<()> {
        let _ = crate::FileSystem::remove_file(self, target_path.as_path());
        let (inode_of_parent, maybe_inode_of_file, name_of_file) =
            self.insert_inode(target_path.as_path())?;

        let inode_of_parent = match inode_of_parent {
            InodeResolution::Found(a) => a,
            InodeResolution::Redirect(..) => {
                return Err(FsError::InvalidInput);
            }
        };

        match maybe_inode_of_file {
            // The file already exists, then it can not be inserted.
            Some(_inode_of_file) => return Err(FsError::AlreadyExists),

            // The file doesn't already exist; it's OK to create it if
            None => {
                // Write lock.
                let mut fs_lock = self.inner.write().map_err(|_| FsError::Lock)?;

                // Read the metadata or generate a dummy one
                let meta = match fs.metadata(&target_path) {
                    Ok(meta) => meta,
                    _ => {
                        let time = time();
                        Metadata {
                            ft: FileType {
                                file: true,
                                ..Default::default()
                            },
                            accessed: time,
                            created: time,
                            modified: time,
                            len: 0,
                        }
                    }
                };

                // Creating the file in the storage.
                let inode_of_file = fs_lock.storage.vacant_entry().key();
                let real_inode_of_file = fs_lock.storage.insert(Node::ArcFile(ArcFileNode {
                    inode: inode_of_file,
                    name: name_of_file,
                    fs,
                    path: source_path,
                    metadata: meta,
                }));

                assert_eq!(
                    inode_of_file, real_inode_of_file,
                    "new file inode should have been correctly calculated",
                );

                // Adding the new directory to its parent.
                fs_lock.add_child_to_node(inode_of_parent, inode_of_file)?;

                inode_of_file
            }
        };
        Ok(())
    }

    /// Inserts a arc file into the file system that references another file
    /// in another file system (does not copy the real data)
    pub fn insert_arc_file(
        &self,
        target_path: PathBuf,
        fs: Arc<dyn crate::FileSystem + Send + Sync>,
    ) -> Result<()> {
        self.insert_arc_file_at(target_path.clone(), fs, target_path)
    }

    /// Inserts a arc directory into the file system that references another file
    /// in another file system (does not copy the real data)
    pub fn insert_arc_directory_at(
        &self,
        target_path: PathBuf,
        other: Arc<dyn crate::FileSystem + Send + Sync>,
        source_path: PathBuf,
    ) -> Result<()> {
        let _ = crate::FileSystem::remove_dir(self, target_path.as_path());
        let (inode_of_parent, maybe_inode_of_file, name_of_file) =
            self.insert_inode(target_path.as_path())?;

        let inode_of_parent = match inode_of_parent {
            InodeResolution::Found(a) => a,
            InodeResolution::Redirect(..) => {
                return Err(FsError::InvalidInput);
            }
        };

        match maybe_inode_of_file {
            // The file already exists, then it can not be inserted.
            Some(_inode_of_file) => return Err(FsError::AlreadyExists),

            // The file doesn't already exist; it's OK to create it if
            None => {
                // Write lock.
                let mut fs_lock = self.inner.write().map_err(|_| FsError::Lock)?;

                // Creating the file in the storage.
                let inode_of_file = fs_lock.storage.vacant_entry().key();
                let real_inode_of_file =
                    fs_lock.storage.insert(Node::ArcDirectory(ArcDirectoryNode {
                        inode: inode_of_file,
                        name: name_of_file,
                        fs: other,
                        path: source_path,
                        metadata: {
                            let time = time();
                            Metadata {
                                ft: FileType {
                                    file: true,
                                    ..Default::default()
                                },
                                accessed: time,
                                created: time,
                                modified: time,
                                len: 0,
                            }
                        },
                    }));

                assert_eq!(
                    inode_of_file, real_inode_of_file,
                    "new file inode should have been correctly calculated",
                );

                // Adding the new directory to its parent.
                fs_lock.add_child_to_node(inode_of_parent, inode_of_file)?;

                inode_of_file
            }
        };
        Ok(())
    }

    /// Inserts a arc directory into the file system that references another file
    /// in another file system (does not copy the real data)
    pub fn insert_arc_directory(
        &self,
        target_path: PathBuf,
        other: Arc<dyn crate::FileSystem + Send + Sync>,
    ) -> Result<()> {
        self.insert_arc_directory_at(target_path.clone(), other, target_path)
    }

    /// Inserts a arc file into the file system that references another file
    /// in another file system (does not copy the real data)
    pub fn insert_device_file(
        &self,
        path: PathBuf,
        file: Box<dyn crate::VirtualFile + Send + Sync>,
    ) -> Result<()> {
        let _ = crate::FileSystem::remove_file(self, path.as_path());
        let (inode_of_parent, maybe_inode_of_file, name_of_file) =
            self.insert_inode(path.as_path())?;

        let inode_of_parent = match inode_of_parent {
            InodeResolution::Found(a) => a,
            InodeResolution::Redirect(..) => {
                // TODO: should remove the inode again!
                return Err(FsError::InvalidInput);
            }
        };

        if let Some(_inode_of_file) = maybe_inode_of_file {
            // TODO: restore previous inode?
            return Err(FsError::AlreadyExists);
        }
        // Write lock.
        let mut fs_lock = self.inner.write().map_err(|_| FsError::Lock)?;

        // Creating the file in the storage.
        let inode_of_file = fs_lock.storage.vacant_entry().key();
        let real_inode_of_file = fs_lock.storage.insert(Node::CustomFile(CustomFileNode {
            inode: inode_of_file,
            name: name_of_file,
            file: Mutex::new(file),
            metadata: {
                let time = time();
                Metadata {
                    ft: FileType {
                        file: true,
                        ..Default::default()
                    },
                    accessed: time,
                    created: time,
                    modified: time,
                    len: 0,
                }
            },
        }));

        assert_eq!(
            inode_of_file, real_inode_of_file,
            "new file inode should have been correctly calculated",
        );

        // Adding the new directory to its parent.
        fs_lock.add_child_to_node(inode_of_parent, inode_of_file)?;

        Ok(())
    }

    fn insert_inode(
        &self,
        path: &Path,
    ) -> Result<(InodeResolution, Option<InodeResolution>, OsString)> {
        // Read lock.
        let fs = self.inner.read().map_err(|_| FsError::Lock)?;

        // Check the path has a parent.
        let parent_of_path = path.parent().ok_or(FsError::BaseNotDirectory)?;

        // Check the file name.
        let name_of_file = path
            .file_name()
            .ok_or(FsError::InvalidInput)?
            .to_os_string();

        // Find the parent inode.
        let inode_of_parent = match fs.inode_of_parent(parent_of_path)? {
            InodeResolution::Found(a) => a,
            InodeResolution::Redirect(fs, parent_path) => {
                return Ok((
                    InodeResolution::Redirect(fs, parent_path),
                    None,
                    name_of_file,
                ));
            }
        };

        // Find the inode of the file if it exists.
        let maybe_inode_of_file = fs
            .as_parent_get_position_and_inode_of_file(inode_of_parent, &name_of_file)?
            .map(|(_nth, inode)| inode);

        Ok((
            InodeResolution::Found(inode_of_parent),
            maybe_inode_of_file,
            name_of_file,
        ))
    }
}

impl crate::FileOpener for FileSystem {
    fn open(
        &self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>> {
        debug!(path=%path.display(), "open");

        let read = conf.read();
        let mut write = conf.write();
        let append = conf.append();
        let mut truncate = conf.truncate();
        let mut create = conf.create();
        let create_new = conf.create_new();

        // If `create_new` is used, `create` and `truncate ` are ignored.
        if create_new {
            create = false;
            truncate = false;
        }

        // To truncate a file, `write` must be used.
        if truncate && !write {
            return Err(FsError::PermissionDenied);
        }

        // `append` is semantically equivalent to `write` + `append`
        // but let's keep them exclusive.
        if append {
            write = false;
        }

        let (inode_of_parent, maybe_inode_of_file, name_of_file) = self.insert_inode(path)?;

        let inode_of_parent = match inode_of_parent {
            InodeResolution::Found(a) => a,
            InodeResolution::Redirect(fs, mut parent_path) => {
                parent_path.push(name_of_file);
                return fs
                    .new_open_options()
                    .options(conf.clone())
                    .open(parent_path);
            }
        };

        let mut cursor = 0u64;
        let inode_of_file = match maybe_inode_of_file {
            // The file already exists, and a _new_ one _must_ be
            // created; it's not OK.
            Some(_inode_of_file) if create_new => return Err(FsError::AlreadyExists),

            // The file already exists; it's OK.
            Some(inode_of_file) => {
                let inode_of_file = match inode_of_file {
                    InodeResolution::Found(a) => a,
                    InodeResolution::Redirect(fs, path) => {
                        return fs.new_open_options().options(conf.clone()).open(path);
                    }
                };

                // Write lock.
                let mut fs = self.inner.write().map_err(|_| FsError::Lock)?;

                let inode = fs.storage.get_mut(inode_of_file);
                match inode {
                    Some(Node::File(FileNode { metadata, file, .. })) => {
                        // Update the accessed time.
                        metadata.accessed = time();

                        // Truncate if needed.
                        if truncate {
                            file.truncate();
                            metadata.len = 0;
                        }

                        // Move the cursor to the end if needed.
                        if append {
                            cursor = file.len() as u64;
                        }
                    }

                    Some(Node::OffloadedFile(OffloadedFileNode { metadata, file, .. })) => {
                        // Update the accessed time.
                        metadata.accessed = time();

                        // Truncate if needed.
                        if truncate {
                            file.truncate();
                            metadata.len = 0;
                        }

                        // Move the cursor to the end if needed.
                        if append {
                            cursor = file.len();
                        }
                    }

                    Some(Node::ReadOnlyFile(node)) => {
                        // Update the accessed time.
                        node.metadata.accessed = time();

                        // Truncate if needed.
                        if truncate || append {
                            return Err(FsError::PermissionDenied);
                        }
                    }

                    Some(Node::CustomFile(node)) => {
                        // Update the accessed time.
                        node.metadata.accessed = time();

                        // Truncate if needed.
                        let mut file = node.file.lock().unwrap();
                        if truncate {
                            file.set_len(0)?;
                            node.metadata.len = 0;
                        }

                        // Move the cursor to the end if needed.
                        if append {
                            cursor = file.size();
                        }
                    }

                    Some(Node::ArcFile(node)) => {
                        // Update the accessed time.
                        node.metadata.accessed = time();

                        let mut file = node
                            .fs
                            .new_open_options()
                            .read(read)
                            .write(write)
                            .append(append)
                            .truncate(truncate)
                            .create(create)
                            .create_new(create_new)
                            .open(node.path.as_path())?;

                        // Truncate if needed.
                        if truncate {
                            file.set_len(0)?;
                            node.metadata.len = 0;
                        }

                        // Move the cursor to the end if needed.
                        if append {
                            cursor = file.size();
                        }
                    }

                    None => return Err(FsError::EntryNotFound),
                    _ => return Err(FsError::NotAFile),
                }

                inode_of_file
            }

            // The file doesn't already exist; it's OK to create it if:
            // 1. `create_new` is used with `write` or `append`,
            // 2. `create` is used with `write` or `append`.
            None if (create_new || create) && (create_new || write || append) => {
                // Write lock.
                let mut fs = self.inner.write().map_err(|_| FsError::Lock)?;

                let metadata = {
                    let time = time();
                    Metadata {
                        ft: FileType {
                            file: true,
                            ..Default::default()
                        },
                        accessed: time,
                        created: time,
                        modified: time,
                        len: 0,
                    }
                };
                let inode_of_file = fs.storage.vacant_entry().key();

                // We might be in optimized mode
                let file = if let Some(offload) = fs.backing_offload.clone() {
                    let file = OffloadedFile::new(fs.limiter.clone(), offload);
                    Node::OffloadedFile(OffloadedFileNode {
                        inode: inode_of_file,
                        name: name_of_file,
                        file,
                        metadata,
                    })
                } else {
                    let file = File::new(fs.limiter.clone());
                    Node::File(FileNode {
                        inode: inode_of_file,
                        name: name_of_file,
                        file,
                        metadata,
                    })
                };

                // Creating the file in the storage.
                let real_inode_of_file = fs.storage.insert(file);

                assert_eq!(
                    inode_of_file, real_inode_of_file,
                    "new file inode should have been correctly calculated",
                );

                // Adding the new directory to its parent.
                fs.add_child_to_node(inode_of_parent, inode_of_file)?;

                inode_of_file
            }

            None if (create_new || create) => return Err(FsError::PermissionDenied),

            None => return Err(FsError::EntryNotFound),
        };

        Ok(Box::new(FileHandle::new(
            inode_of_file,
            self.clone(),
            read,
            write || append || truncate,
            append,
            cursor,
        )))
    }
}

#[cfg(test)]
mod test_file_opener {
    use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

    use crate::{mem_fs::*, FileSystem as FS, FsError};
    use std::io;

    macro_rules! path {
        ($path:expr) => {
            std::path::Path::new($path)
        };
    }

    #[tokio::test]
    async fn test_create_new_file() {
        let fs = FileSystem::default();

        assert!(
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(path!("/foo.txt"))
                .is_ok(),
            "creating a new file",
        );

        {
            let fs_inner = fs.inner.read().unwrap();

            assert_eq!(fs_inner.storage.len(), 2, "storage has the new file");
            assert!(
                matches!(
                    fs_inner.storage.get(ROOT_INODE),
                    Some(Node::Directory(DirectoryNode {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    })) if name == "/" && children == &[1]
                ),
                "`/` contains `foo.txt`",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(1),
                    Some(Node::File(FileNode {
                        inode: 1,
                        name,
                        ..
                    })) if name == "foo.txt"
                ),
                "`foo.txt` exists and is a file",
            );
        }

        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(path!("/foo.txt")),
                Err(FsError::AlreadyExists)
            ),
            "creating a new file that already exist",
        );

        assert_eq!(
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(path!("/foo/bar.txt"))
                .map(|_| ()),
            Err(FsError::EntryNotFound),
            "creating a file in a directory that doesn't exist",
        );

        assert_eq!(fs.remove_file(path!("/foo.txt")), Ok(()), "removing a file");

        assert!(
            fs.new_open_options()
                .write(false)
                .create_new(true)
                .open(path!("/foo.txt"))
                .is_ok(),
            "creating a file without the `write` option",
        );
    }

    #[tokio::test]
    async fn test_truncate_a_read_only_file() {
        let fs = FileSystem::default();

        assert!(
            matches!(
                fs.new_open_options()
                    .write(false)
                    .truncate(true)
                    .open(path!("/foo.txt")),
                Err(FsError::PermissionDenied),
            ),
            "truncating a read-only file",
        );
    }

    #[tokio::test]
    async fn test_truncate() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.write(b"foobar").await, Ok(6)),
            "writing `foobar` at the end of the file",
        );

        assert!(
            matches!(file.seek(io::SeekFrom::Current(0)).await, Ok(6)),
            "checking the current position is 6",
        );
        assert!(
            matches!(file.seek(io::SeekFrom::End(0)).await, Ok(6)),
            "checking the size is 6",
        );

        let mut file = fs
            .new_open_options()
            .write(true)
            .truncate(true)
            .open(path!("/foo.txt"))
            .expect("failed to open + truncate `foo.txt`");

        assert!(
            matches!(file.seek(io::SeekFrom::Current(0)).await, Ok(0)),
            "checking the current position is 0",
        );
        assert!(
            matches!(file.seek(io::SeekFrom::End(0)).await, Ok(0)),
            "checking the size is 0",
        );
    }

    #[tokio::test]
    async fn test_append() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.write(b"foobar").await, Ok(6)),
            "writing `foobar` at the end of the file",
        );

        assert!(
            matches!(file.seek(io::SeekFrom::Current(0)).await, Ok(6)),
            "checking the current position is 6",
        );
        assert!(
            matches!(file.seek(io::SeekFrom::End(0)).await, Ok(6)),
            "checking the size is 6",
        );

        let mut file = fs
            .new_open_options()
            .append(true)
            .open(path!("/foo.txt"))
            .expect("failed to open `foo.txt`");

        assert!(
            matches!(file.seek(io::SeekFrom::Current(0)).await, Ok(0)),
            "checking the current position in append-mode is 0",
        );
        assert!(
            matches!(file.seek(io::SeekFrom::Start(0)).await, Ok(0)),
            "trying to rewind in append-mode",
        );
        assert!(matches!(file.write(b"baz").await, Ok(3)), "writing `baz`");

        let mut file = fs
            .new_open_options()
            .read(true)
            .open(path!("/foo.txt"))
            .expect("failed to open `foo.txt");

        assert!(
            matches!(file.seek(io::SeekFrom::Current(0)).await, Ok(0)),
            "checking the current position is read-mode is 0",
        );

        let mut string = String::new();
        assert!(
            matches!(file.read_to_string(&mut string).await, Ok(9)),
            "reading the entire `foo.txt` file",
        );
        assert_eq!(
            string, "foobarbaz",
            "checking append-mode is ignoring seek operations",
        );
    }

    #[tokio::test]
    async fn test_opening_a_file_that_already_exists() {
        let fs = FileSystem::default();

        assert!(
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(path!("/foo.txt"))
                .is_ok(),
            "creating a _new_ file",
        );

        assert!(
            matches!(
                fs.new_open_options()
                    .create_new(true)
                    .open(path!("/foo.txt")),
                Err(FsError::AlreadyExists),
            ),
            "creating a _new_ file that already exists",
        );

        assert!(
            fs.new_open_options()
                .read(true)
                .open(path!("/foo.txt"))
                .is_ok(),
            "opening a file that already exists",
        );
    }
}
