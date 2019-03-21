use crate::vfs::file_like::FileLike;
use crate::vfs::vfs_header::{header_from_bytes, ArchiveType, CompressionType};
use crate::vfs::virtual_file::VirtualFile;
use std::collections::BTreeMap;
use std::io;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use tar::EntryType;
use zbox::{init_env, OpenOptions, Repo, RepoOpener};

pub type Fd = i32;

pub struct Vfs {
    repo: Repo,
    pub fd_map: BTreeMap<Fd, Rc<dyn FileLike>>,
    pub import_errors: Vec<VfsAggregateError>,
}

impl Vfs {
    /// Like `VfsBacking::from_tar_bytes` except it also decompresses from the zstd format.
    pub fn from_tar_zstd_bytes<Reader: Read>(tar_bytes: Reader) -> Result<Self, failure::Error> {
        let result = zstd::decode_all(tar_bytes);
        let decompressed_data = result.unwrap();
        Vfs::from_tar_bytes(&decompressed_data[..])
    }

    /// Match on the type of the compressed-archive and select the correct unpack method
    pub fn from_compressed_bytes(compressed_data_slice: &[u8]) -> Result<Self, failure::Error> {
        let data_bytes = &compressed_data_slice[4..];
        match header_from_bytes(compressed_data_slice)? {
            (_, CompressionType::ZSTD, ArchiveType::TAR) => Vfs::from_tar_zstd_bytes(data_bytes),
            (_, CompressionType::NONE, ArchiveType::TAR) => Vfs::from_tar_bytes(data_bytes),
        }
    }

    /// Create a vfs from raw bytes in tar format
    pub fn from_tar_bytes<Reader: Read>(tar_bytes: Reader) -> Result<Self, failure::Error> {
        init_env();
        let mut repo = RepoOpener::new()
            .create(true)
            .open("mem://wasmer_fs", "")
            .unwrap();

        let mut fd_map: BTreeMap<Fd, Rc<dyn FileLike>> = BTreeMap::new();

        // TODO: What to do about the creation of the device files?
        let _ = repo.create_dir(PathBuf::from("/dev/"));
        let stdin = repo.create_file(PathBuf::from("/dev/stdin"))?;
        let stdout = repo.create_file(PathBuf::from("/dev/stdout"))?;
        let stderr = repo.create_file(PathBuf::from("/dev/stderr"))?;

        use crate::vfs::device_file;
        fd_map.insert(0, Rc::new(device_file::Stdin {}));
        fd_map.insert(1, Rc::new(device_file::Stdin {})); // TODO FIX ME
        fd_map.insert(2, Rc::new(device_file::Stdin {}));

        let errors = tar::Archive::new(tar_bytes)
            .entries()?
            .map(|entry| {
                let mut entry: tar::Entry<Reader> = entry?;
                let path = entry.path()?;
                let path = convert_to_absolute_path(path);
                let result = match (entry.header().entry_type(), path.parent()) {
                    (EntryType::Regular, Some(parent)) => {
                        if let Err(e) = repo.create_dir_all(parent) {
                            if e == zbox::Error::AlreadyExists || e == zbox::Error::IsRoot {
                            } else {
                                return Err(VfsAggregateError::ZboxError(e));
                            }
                        } else {
                        }
                        let mut file = repo.create_file(&path)?;
                        if entry.header().size().unwrap_or(0) > 0 {
                            io::copy(&mut entry, &mut file)?;
                            file.finish()?;
                        }
                    }
                    (EntryType::Directory, _) => {
                        if let Err(e) = repo.create_dir_all(path) {
                            if e == zbox::Error::AlreadyExists || e == zbox::Error::IsRoot {
                            } else {
                                return Err(VfsAggregateError::ZboxError(e));
                            }
                        } else {
                        }
                    }
                    _ => return Err(VfsAggregateError::UnsupportedFileType),
                };
                Ok(())
            })
            .collect::<Vec<Result<(), VfsAggregateError>>>();

        let vfs = Vfs {
            repo,
            fd_map,
            import_errors: vec![],
        };
        Ok(vfs)
    }

    /// like read(2), will read the data for the file descriptor
    pub fn read_file(&mut self, fd: Fd, buf: &mut [u8]) -> Result<usize, failure::Error> {
        let mut data = self
            .fd_map
            .get_mut(&fd)
            .ok_or(VfsError::FileDescriptorNotExist(fd))?;
        match Rc::get_mut(&mut data) {
            Some(file) => file.read(buf),
            None => Err(VfsError::CouldNotGetMutableReferenceToFile.into()),
        }
    }

    /// like open(2), creates a file descriptor for the path if it exists
    pub fn open_file<P: AsRef<Path>>(&mut self, path: P) -> Result<Fd, failure::Error> {
        let path = convert_to_absolute_path(path);
        let file = OpenOptions::new().write(true).open(&mut self.repo, &path)?;
        let mut next_lowest_fd = 0;
        for (fd, _) in self.fd_map.iter() {
            if *fd == next_lowest_fd {
                next_lowest_fd += 1;
            } else if *fd < next_lowest_fd {
                panic!("Should not be here.");
            } else {
                break;
            }
        }
        let virtual_file = VirtualFile::new(file);
        self.fd_map.insert(next_lowest_fd, Rc::new(virtual_file));
        Ok(next_lowest_fd)
    }

    fn next_lowest(&self) -> Fd {
        let mut next_lowest_fd = 0;
        for (fd, _) in self.fd_map.iter() {
            if *fd == next_lowest_fd {
                next_lowest_fd += 1;
            } else if *fd < next_lowest_fd {
                panic!("Should not be here.");
            } else {
                break;
            }
        }
        next_lowest_fd
    }

    /// like dup2, but better for this abstraction layer
    pub fn duplicate_handle(&mut self, handle: &Fd) -> Fd {
        let dup = match self.fd_map.get(handle) {
            Some(file) => file.clone(),
            None => panic!(),
        };
        let new_handle = self.next_lowest();
        assert!(!self.fd_map.contains_key(&new_handle));
        self.fd_map.insert(new_handle, dup);
        new_handle
    }

    /// like dup2
    pub fn duplicate_file_descriptor(
        &mut self,
        source_fd: Fd,
        target_fd: Fd,
    ) -> Result<Fd, failure::Error> {
        // find the file and check if the target descriptor is already open
        let (target_is_open_file, file) = {
            let fd_map = &self.fd_map;
            let source_file = fd_map.get(&source_fd);
            let target_file = fd_map.get(&target_fd);
            match (source_file, target_file) {
                // the source is not already open
                (None, _) => Err(VfsError::SourceFileDescriptorDoesNotExist),
                // the target fd is already open, close it first
                (_, Some(file)) => Ok((true, file.clone())),
                // normal case
                (Some(file), None) => Ok((false, file.clone())),
            }
        }?;
        // if the target fd is already open, close it first
        if target_is_open_file {
            let fd_map = &mut self.fd_map;
            fd_map.remove(&target_fd);
            fd_map.insert(target_fd, file.clone());
        } else {
            let fd_map = &mut self.fd_map;
            fd_map.insert(target_fd, file.clone());
        }
        Ok(target_fd)
    }

    /// close
    pub fn close(&mut self, fd: &Fd) -> Result<(), failure::Error> {
        let result = if let Some(file) = self.fd_map.remove(fd) {
            file.close()
        } else {
            // this file did not exist in the virtual file system, maybe throw an error in the future
            Ok(())
        };
        assert!(!self.fd_map.contains_key(&fd));
        result
    }

    /// get metadata with file descriptor
    pub fn get_file_metadata(
        &self,
        fd: &Fd,
    ) -> Result<crate::vfs::file_like::Metadata, failure::Error> {
        match self.fd_map.get(&fd) {
            None => Err(VfsError::FileWithFileDescriptorNotExist(*fd).into()),
            Some(file) => {
                //                let file = file.clone();
                let file = file.clone();
                file.metadata()
            }
        }
    }

    /// get metadata with path
    pub fn get_path_metadata<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<zbox::Metadata, failure::Error> {
        let path = convert_to_absolute_path(path);
        self.repo.metadata(path).map_err(|e| e.into())
    }

    pub fn make_dir<P: AsRef<Path>>(&mut self, path: P) -> Result<(), failure::Error> {
        self.repo.create_dir_all(path).map_err(|e| e.into())
    }

    /// write to a file with the file descriptor
    pub fn write_file(
        &mut self,
        fd: Fd,
        buf: &[u8],
        count: usize,
        offset: usize,
    ) -> Result<usize, failure::Error> {
        let mut file = self
            .fd_map
            .get_mut(&fd)
            .ok_or(VfsError::FileWithFileDescriptorNotExist(fd))?;
        let file = Rc::get_mut(&mut file);
        match file {
           Some(file) =>  file.write(buf, count, offset),
            None => Ok(count) // BAD!!! Switch to Rc<RefCell>
        }

    }
}

#[derive(Debug, Fail)]
pub enum VfsError {
    #[fail(display = "File with file descriptor \"{}\" does not exist.", _0)]
    FileWithFileDescriptorNotExist(Fd),
    #[fail(display = "File descriptor does not exist.")]
    FileDescriptorNotExist(Fd),
    #[fail(display = "Source file descriptor does not exist.")]
    SourceFileDescriptorDoesNotExist,
    #[fail(display = "Target file descriptor already exists.")]
    TargetFileDescriptorAlreadyExists,
    #[fail(display = "Could not get a mutable reference to the file because it is in use.")]
    CouldNotGetMutableReferenceToFile,
}

#[derive(Debug, Fail)]
pub enum VfsAggregateError {
    #[fail(display = "Entry error.")]
    EntryError(std::io::Error),
    #[fail(display = "IO error.")]
    IoError(std::io::Error),
    #[fail(display = "Zbox error.")]
    ZboxError(zbox::Error),
    #[fail(display = "Unsupported file type.")]
    UnsupportedFileType,
}

impl std::convert::From<std::io::Error> for VfsAggregateError {
    fn from(error: std::io::Error) -> VfsAggregateError {
        VfsAggregateError::EntryError(error)
    }
}

impl std::convert::From<zbox::Error> for VfsAggregateError {
    fn from(error: zbox::Error) -> VfsAggregateError {
        VfsAggregateError::ZboxError(error)
    }
}

fn convert_to_absolute_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let path = path.as_ref();
    if path.is_relative() {
        std::path::PathBuf::from("/").join(path)
    } else {
        path.to_path_buf()
    }
}

#[cfg(test)]
mod open_test {
    use crate::vfs::vfs::Vfs;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn open_and_close_files() {
        // SETUP: create temp dir and files
        let tmp_dir = tempdir::TempDir::new("open_files").unwrap();
        let file_path = tmp_dir.path().join("foo.txt");
        let mut tmp_file = File::create(file_path.clone()).unwrap();
        writeln!(tmp_file, "foo foo foo").unwrap();
        let tar_data = vec![];
        let mut ar = tar::Builder::new(tar_data);
        ar.append_path_with_name(file_path, "foo.txt").unwrap();
        let archive = ar.into_inner().unwrap();
        // SETUP: create virtual filesystem with tar data
        let vfs_result = Vfs::from_tar_bytes(&archive[..]);
        // ASSERT:
        assert!(
            vfs_result.is_ok(),
            "Failed to create file system from archive"
        );
        let mut vfs = vfs_result.unwrap();
        // open the file, get a file descriptor
        let open_result = vfs.open_file("foo.txt");
        assert!(
            open_result.is_ok(),
            "Failed to open file in the virtual filesystem."
        );
        // open the same file twice, and expect different descriptors
        let fd_1 = open_result.unwrap();
        let open_result_2 = vfs.open_file("foo.txt");
        assert!(
            open_result_2.is_ok(),
            "Failed to open the same file twice in the virtual filesystem."
        );
        let fd_2 = open_result_2.unwrap();
        assert_ne!(fd_1, fd_2, "Open produced the same file descriptor twice.");
        assert!(fd_2 > 0, "File descriptor was less than 0.");

        // try opening as absolute path
        let open_result_3 = vfs.open_file("/foo.txt");
        assert!(
            open_result_3.is_ok(),
            "Failed to open the same file twice in the virtual filesystem."
        );
        let fd_3 = open_result_3.unwrap();
        assert!(fd_3 > 0, "File descriptor was less than 0.");

        let close_result = vfs.close(fd_3);
        assert!(close_result.is_ok(), "Close failed.");

        // re-open the file, assert the file descriptor is the same
        let open_result_4 = vfs.open_file("/foo.txt");
        assert!(
            open_result_4.is_ok(),
            "Failed to close a file, then the file again in the virtual filesystem."
        );
        let fd_4 = open_result_4.unwrap();

        assert_eq!(
            fd_3, fd_4,
            "Expected the lowest available file descriptor to be used."
        );

        // close a lower file descriptor
        let close_result_2 = vfs.close(fd_1);
        assert!(close_result_2.is_ok(), "Close failed");

        // re-open the file, assert the file descriptor is the same
        let open_result_5 = vfs.open_file("/foo.txt");
        assert!(
            open_result_5.is_ok(),
            "Failed to open a file, open more files, then close the file, and then open it again and get the lowest file descriptor in in the virtual filesystem."
        );
        let fd_5 = open_result_5.unwrap();
        assert_eq!(
            fd_5, fd_1,
            "Expected the lowest available file descriptor to be used."
        );

        // re-open the file, assert the file descriptor is correct
        let open_result_6 = vfs.open_file("/foo.txt");
        assert!(open_result_6.is_ok());
        // we re-opened a file which took the recently opened low file descriptor. Now we get the next lowest file descriptor.
        let fd_6 = open_result_6.unwrap();
        assert_eq!(fd_6, fd_4 + 1);
    }

    #[test]
    fn open_non_existent_file() {
        // SETUP: create temp dir and files
        let tmp_dir = tempdir::TempDir::new("open_non_existent_file").unwrap();
        let file_path = tmp_dir.path().join("foo.txt");
        let mut tmp_file = File::create(file_path.clone()).unwrap();
        writeln!(tmp_file, "foo foo foo").unwrap();
        let tar_data = vec![];
        let mut ar = tar::Builder::new(tar_data);
        ar.append_path_with_name(file_path, "foo.txt").unwrap();
        let archive = ar.into_inner().unwrap();
        // SETUP: create virtual filesystem with tar data
        let vfs_result = Vfs::from_tar_bytes(&archive[..]);
        // ASSERT:
        assert!(
            vfs_result.is_ok(),
            "Failed to create file system from archive"
        );
        let mut vfs = vfs_result.unwrap();
        // read the file
        let open_result = vfs.open_file("foo.txt");
        assert!(open_result.is_ok(), "Failed to read file from vfs");
        // open a non-existent file
        let open_result_2 = vfs.open_file("bar.txt");
        assert!(
            open_result_2.is_err(),
            "Somehow opened a non-existent file."
        );
    }
}

#[cfg(test)]
mod read_test {
    use crate::vfs::vfs::Vfs;
    use std::fs;
    use std::fs::File;
    use std::io::Write;
    use tempdir;

    #[test]
    fn empty_archive() {
        // SETUP: create temp dir and files
        let empty_archive = vec![];
        // SETUP: create virtual filesystem with tar data
        let vfs_result = Vfs::from_tar_bytes(&empty_archive[..]);
        // ASSERT:
        assert!(
            vfs_result.is_ok(),
            "Failed to create file system from empty archive"
        );
        // assert import errors
        let vfs = vfs_result.unwrap();
        assert_eq!(
            vfs.import_errors.len(),
            0,
            "Expected no import errors. Found {} errors.",
            vfs.import_errors.len()
        );
    }

    #[test]
    fn single_file_archive() {
        // SETUP: create temp dir and files
        let tmp_dir = tempdir::TempDir::new("single_file_archive").unwrap();
        let foo_file_path = tmp_dir.path().join("foo.txt");
        let mut foo_tmp_file = File::create(foo_file_path.clone()).unwrap();
        writeln!(foo_tmp_file, "foo foo foo").unwrap();
        let tar_data = vec![];
        let mut ar = tar::Builder::new(tar_data);
        ar.append_path_with_name(foo_file_path, "foo.txt").unwrap();
        let archive = ar.into_inner().unwrap();
        // SETUP: create virtual filesystem with tar data
        let vfs_result = Vfs::from_tar_bytes(&archive[..]);
        // ASSERT:
        assert!(
            vfs_result.is_ok(),
            "Failed to create file system from archive"
        );
        let mut vfs = vfs_result.unwrap();
        // read the file
        let fd = vfs.open_file("foo.txt").unwrap();
        let mut actual_data: [u8; 12] = [0; 12];
        let read_result = vfs.read_file(fd, &mut actual_data);
        assert!(read_result.is_ok(), "Failed to read file from vfs");
        let expected_data = "foo foo foo\n".as_bytes();
        assert_eq!(actual_data, expected_data, "Contents were not equal");

        // assert import errors
        assert_eq!(
            vfs.import_errors.len(),
            0,
            "Expected no import errors. Found {} errors.",
            vfs.import_errors.len()
        );
    }

    #[test]
    fn two_files_in_archive() {
        // SETUP: create temp dir and files
        let tmp_dir = tempdir::TempDir::new("two_files_in_archive").unwrap();
        let foo_file_path = tmp_dir.path().join("foo.txt");
        let bar_file_path = tmp_dir.path().join("bar.txt");
        let mut foo_tmp_file = File::create(foo_file_path.clone()).unwrap();
        let mut bar_tmp_file = File::create(bar_file_path.clone()).unwrap();
        writeln!(foo_tmp_file, "foo foo foo").unwrap();
        writeln!(bar_tmp_file, "bar bar").unwrap();
        let tar_data = vec![];
        let mut ar = tar::Builder::new(tar_data);
        ar.append_path_with_name(foo_file_path, "foo.txt").unwrap();
        ar.append_path_with_name(bar_file_path, "bar.txt").unwrap();
        let archive = ar.into_inner().unwrap();
        // SETUP: create virtual filesystem with tar data
        let vfs_result = Vfs::from_tar_bytes(&archive[..]);
        // ASSERT:
        assert!(
            vfs_result.is_ok(),
            "Failed to create file system from archive"
        );
        let mut vfs = vfs_result.unwrap();
        // read the file
        let foo_fd = vfs.open_file("foo.txt").unwrap();
        let bar_fd = vfs.open_file("bar.txt").unwrap();
        let mut foo_actual_data: [u8; 12] = [0; 12];
        let foo_read_result = vfs.read_file(foo_fd, &mut foo_actual_data);
        let mut bar_actual_data: [u8; 8] = [0; 8];
        let bar_read_result = vfs.read_file(bar_fd, &mut bar_actual_data);
        assert!(foo_read_result.is_ok(), "Failed to read foo.txt from vfs");
        assert!(bar_read_result.is_ok(), "Failed to read bar.txt from vfs");
        let foo_expected_data: &[u8; 12] = b"foo foo foo\n";
        let bar_expected_data: &[u8; 8] = b"bar bar\n";
        assert_eq!(
            &foo_actual_data, foo_expected_data,
            "Contents of `foo.txt` is not correct"
        );
        assert_eq!(
            &bar_actual_data, bar_expected_data,
            "Contents of `bar.txt` is not correct"
        );
        // assert import errors
        assert_eq!(
            vfs.import_errors.len(),
            0,
            "Expected no import errors. Found {} errors.",
            vfs.import_errors.len()
        );
    }

    #[test]
    fn two_nested_files_in_archive() {
        // SETUP: create temp dir and files
        let tmp_dir = tempdir::TempDir::new("two_nested_files_in_archive").unwrap();
        let baz_dir_path = tmp_dir.path().join("foo").join("bar");
        fs::create_dir_all(baz_dir_path.clone()).unwrap();
        let quuz_dir_path = tmp_dir.path().join("qux").join("quuz");
        fs::create_dir_all(quuz_dir_path.clone()).unwrap();
        let baz_file_path = baz_dir_path.join("baz.txt");
        let quuz_file_path = quuz_dir_path.join("quuz.txt");
        let mut baz_tmp_file = File::create(baz_file_path.clone()).unwrap();
        let mut quuz_tmp_file = File::create(quuz_file_path.clone()).unwrap();
        writeln!(baz_tmp_file, "baz baz baz baz").unwrap();
        writeln!(quuz_tmp_file, "quuz").unwrap();
        let tar_data = vec![];
        let mut ar = tar::Builder::new(tar_data);
        ar.append_path_with_name(baz_file_path, "foo/bar/baz.txt")
            .unwrap();
        ar.append_path_with_name(quuz_file_path, "qux/quux/quuz.txt")
            .unwrap();
        let archive = ar.into_inner().unwrap();
        // SETUP: create virtual filesystem with tar data
        let vfs_result = Vfs::from_tar_bytes(&archive[..]);
        // ASSERT:
        assert!(
            vfs_result.is_ok(),
            "Failed to create file system from archive"
        );
        let mut vfs = vfs_result.unwrap();
        // read the file
        let baz_fd = vfs.open_file("foo/bar/baz.txt").unwrap();
        let quuz_fd = vfs.open_file("qux/quux/quuz.txt").unwrap();
        let mut baz_actual_data: [u8; 16] = [0; 16];
        let baz_read_result = vfs.read_file(baz_fd, &mut baz_actual_data);
        let mut quuz_actual_data: [u8; 5] = [0; 5];
        let quuz_read_result = vfs.read_file(quuz_fd, &mut quuz_actual_data);
        assert!(
            baz_read_result.is_ok(),
            "Failed to read foo/bar/baz.txt from vfs"
        );
        assert!(
            quuz_read_result.is_ok(),
            "Failed to read qux/quux/quuz.txt from vfs"
        );
        let baz_expected_data: &[u8; 16] = b"baz baz baz baz\n";
        let quuz_expected_data: &[u8; 5] = b"quuz\n";
        assert_eq!(
            &baz_actual_data, baz_expected_data,
            "Contents of `foo/bar/baz.txt` is not correct"
        );
        assert_eq!(
            &quuz_actual_data, quuz_expected_data,
            "Contents of `qux/quux/quuz.txt` is not correct"
        );
        // assert import errors
        assert_eq!(
            vfs.import_errors.len(),
            0,
            "Expected no import errors. Found {} errors.",
            vfs.import_errors.len()
        );
    }
}

#[cfg(test)]
mod dup_test {
    use crate::vfs::vfs::{Fd, Vfs};
    use std::fs::File;
    use std::io::Write;
    use std::sync::Arc;

    #[test]
    fn duplicates_file_descriptor() {
        // SETUP: create temp dir and files
        let tmp_dir = tempdir::TempDir::new("two_files_in_archive").unwrap();
        let foo_file_path = tmp_dir.path().join("foo.txt");
        let bar_file_path = tmp_dir.path().join("bar.txt");
        let mut foo_tmp_file = File::create(foo_file_path.clone()).unwrap();
        let mut bar_tmp_file = File::create(bar_file_path.clone()).unwrap();
        writeln!(foo_tmp_file, "foo foo foo").unwrap();
        writeln!(bar_tmp_file, "bar bar").unwrap();
        let tar_data = vec![];
        let mut ar = tar::Builder::new(tar_data);
        ar.append_path_with_name(foo_file_path, "foo.txt").unwrap();
        ar.append_path_with_name(bar_file_path, "bar.txt").unwrap();
        let archive = ar.into_inner().unwrap();
        // SETUP: create virtual filesystem with tar data
        let vfs_result = Vfs::from_tar_bytes(&archive[..]);
        // ASSERT:
        assert!(
            vfs_result.is_ok(),
            "Failed to create file system from archive"
        );
        let mut vfs = vfs_result.unwrap();

        let source_fd = vfs.open_file("foo.txt").unwrap();
        let target_fd: Fd = 10;
        assert_ne!(
            source_fd, target_fd,
            "Test setup failed. The source descriptor is identical to desired target descriptor."
        );

        let mut fds = vec![];
        fds.push(Arc::new(100));
        fds.push(Arc::new(200));

        let result = vfs.duplicate_file_descriptor(source_fd, target_fd);

        assert!(result.is_ok(), "Failed to duplicated file descriptor.");
        // assert import errors
        assert_eq!(
            vfs.import_errors.len(),
            0,
            "Expected no import errors. Found {} errors.",
            vfs.import_errors.len()
        );
    }
}
