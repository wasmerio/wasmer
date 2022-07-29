use super::*;
use crate::{FileType, FsError, Metadata, OpenOptionsConfig, Result, VirtualFile};
use std::io::{self, Seek};
use std::path::Path;

/// The type that is responsible to open a file.
#[derive(Debug, Clone)]
pub struct FileOpener {
    pub(super) filesystem: FileSystem,
}

impl crate::FileOpener for FileOpener {
    fn open(
        &mut self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>> {
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

        let (inode_of_parent, maybe_inode_of_file, name_of_file) = {
            // Read lock.
            let fs = self
                .filesystem
                .inner
                .try_read()
                .map_err(|_| FsError::Lock)?;

            // Check the path has a parent.
            let parent_of_path = path.parent().ok_or(FsError::BaseNotDirectory)?;

            // Check the file name.
            let name_of_file = path
                .file_name()
                .ok_or(FsError::InvalidInput)?
                .to_os_string();

            // Find the parent inode.
            let inode_of_parent = fs.inode_of_parent(parent_of_path)?;

            // Find the inode of the file if it exists.
            let maybe_inode_of_file = fs
                .as_parent_get_position_and_inode_of_file(inode_of_parent, &name_of_file)?
                .map(|(_nth, inode)| inode);

            (inode_of_parent, maybe_inode_of_file, name_of_file)
        };

        let inode_of_file = match maybe_inode_of_file {
            // The file already exists, and a _new_ one _must_ be
            // created; it's not OK.
            Some(_inode_of_file) if create_new => return Err(FsError::AlreadyExists),

            // The file already exists; it's OK.
            Some(inode_of_file) => {
                // Write lock.
                let mut fs = self
                    .filesystem
                    .inner
                    .try_write()
                    .map_err(|_| FsError::Lock)?;

                let inode = fs.storage.get_mut(inode_of_file);
                match inode {
                    Some(Node::File { metadata, file, .. }) => {
                        // Update the accessed time.
                        metadata.accessed = time();

                        // Truncate if needed.
                        if truncate {
                            file.truncate();
                            metadata.len = 0;
                        }

                        // Move the cursor to the end if needed.
                        if append {
                            file.seek(io::SeekFrom::End(0))?;
                        }
                        // Otherwise, move the cursor to the start.
                        else {
                            file.seek(io::SeekFrom::Start(0))?;
                        }
                    }

                    _ => return Err(FsError::NotAFile),
                }

                inode_of_file
            }

            // The file doesn't already exist; it's OK to create it if:
            // 1. `create_new` is used with `write` or `append`,
            // 2. `create` is used with `write` or `append`.
            None if (create_new || create) && (write || append) => {
                // Write lock.
                let mut fs = self
                    .filesystem
                    .inner
                    .try_write()
                    .map_err(|_| FsError::Lock)?;

                let file = File::new();

                // Creating the file in the storage.
                let inode_of_file = fs.storage.vacant_entry().key();
                let real_inode_of_file = fs.storage.insert(Node::File {
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
                            len: 0,
                        }
                    },
                });

                assert_eq!(
                    inode_of_file, real_inode_of_file,
                    "new file inode should have been correctly calculated",
                );

                // Adding the new directory to its parent.
                fs.add_child_to_node(inode_of_parent, inode_of_file)?;

                inode_of_file
            }

            None => return Err(FsError::PermissionDenied),
        };

        Ok(Box::new(FileHandle::new(
            inode_of_file,
            self.filesystem.clone(),
            read,
            write || append || truncate,
            append,
        )))
    }
}

#[cfg(test)]
mod test_file_opener {
    use crate::{mem_fs::*, FileSystem as FS, FsError};
    use std::io;

    macro_rules! path {
        ($path:expr) => {
            std::path::Path::new($path)
        };
    }

    #[test]
    fn test_create_new_file() {
        let fs = FileSystem::default();

        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(path!("/foo.txt")),
                Ok(_),
            ),
            "creating a new file",
        );

        {
            let fs_inner = fs.inner.read().unwrap();

            assert_eq!(fs_inner.storage.len(), 2, "storage has the new file");
            assert!(
                matches!(
                    fs_inner.storage.get(ROOT_INODE),
                    Some(Node::Directory {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    }) if name == "/" && children == &[1]
                ),
                "`/` contains `foo.txt`",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(1),
                    Some(Node::File {
                        inode: 1,
                        name,
                        ..
                    }) if name == "foo.txt"
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

        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(path!("/foo/bar.txt")),
                Err(FsError::NotAFile),
            ),
            "creating a file in a directory that doesn't exist",
        );

        assert_eq!(fs.remove_file(path!("/foo.txt")), Ok(()), "removing a file");

        assert!(
            matches!(
                fs.new_open_options()
                    .write(false)
                    .create_new(true)
                    .open(path!("/foo.txt")),
                Err(FsError::PermissionDenied),
            ),
            "creating a file without the `write` option",
        );
    }

    #[test]
    fn test_truncate_a_read_only_file() {
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

    #[test]
    fn test_truncate() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.write(b"foobar"), Ok(6)),
            "writing `foobar` at the end of the file",
        );

        assert!(
            matches!(file.seek(io::SeekFrom::Current(0)), Ok(6)),
            "checking the current position is 6",
        );
        assert!(
            matches!(file.seek(io::SeekFrom::End(0)), Ok(6)),
            "checking the size is 6",
        );

        let mut file = fs
            .new_open_options()
            .write(true)
            .truncate(true)
            .open(path!("/foo.txt"))
            .expect("failed to open + truncate `foo.txt`");

        assert!(
            matches!(file.seek(io::SeekFrom::Current(0)), Ok(0)),
            "checking the current position is 0",
        );
        assert!(
            matches!(file.seek(io::SeekFrom::End(0)), Ok(0)),
            "checking the size is 0",
        );
    }

    #[test]
    fn test_append() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.write(b"foobar"), Ok(6)),
            "writing `foobar` at the end of the file",
        );

        assert!(
            matches!(file.seek(io::SeekFrom::Current(0)), Ok(6)),
            "checking the current position is 6",
        );
        assert!(
            matches!(file.seek(io::SeekFrom::End(0)), Ok(6)),
            "checking the size is 6",
        );

        let mut file = fs
            .new_open_options()
            .append(true)
            .open(path!("/foo.txt"))
            .expect("failed to open `foo.txt`");

        assert!(
            matches!(file.seek(io::SeekFrom::Current(0)), Ok(0)),
            "checking the current position in append-mode is 0",
        );
        assert!(
            matches!(file.seek(io::SeekFrom::Start(0)), Ok(0)),
            "trying to rewind in append-mode",
        );
        assert!(matches!(file.write(b"baz"), Ok(3)), "writing `baz`");

        let mut file = fs
            .new_open_options()
            .read(true)
            .open(path!("/foo.txt"))
            .expect("failed to open `foo.txt");

        assert!(
            matches!(file.seek(io::SeekFrom::Current(0)), Ok(0)),
            "checking the current position is read-mode is 0",
        );

        let mut string = String::new();
        assert!(
            matches!(file.read_to_string(&mut string), Ok(9)),
            "reading the entire `foo.txt` file",
        );
        assert_eq!(
            string, "foobarbaz",
            "checking append-mode is ignoring seek operations",
        );
    }

    #[test]
    fn test_opening_a_file_that_already_exists() {
        let fs = FileSystem::default();

        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(path!("/foo.txt")),
                Ok(_),
            ),
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
            matches!(
                fs.new_open_options().read(true).open(path!("/foo.txt")),
                Ok(_),
            ),
            "opening a file that already exists",
        );
    }
}
