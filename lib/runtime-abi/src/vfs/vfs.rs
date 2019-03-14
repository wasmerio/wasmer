use crate::vfs::vfs_header::{header_from_bytes, ArchiveType, CompressionType};
use std::collections::BTreeMap;
use std::io;
use std::io::Read;
use std::path::{Path, PathBuf};
use zbox::{init_env, OpenOptions, Repo, RepoOpener};

pub type Fd = isize;

pub struct Vfs {
    pub repo: Repo,
    pub fd_map: BTreeMap<Fd, zbox::File>, // best because we look for lowest fd
}

impl Vfs {
    /// Like `VfsBacking::from_tar_bytes` except it also decompresses from the zstd format.
    pub fn from_tar_zstd_bytes<Reader: Read>(tar_bytes: Reader) -> Result<Self, failure::Error> {
        let result = zstd::decode_all(tar_bytes);
        let decompressed_data = result.unwrap();
        Vfs::from_tar_bytes(&decompressed_data[..])
    }

    pub fn from_compressed_bytes(compressed_data_slice: &[u8]) -> Result<Self, failure::Error> {
        let data_bytes = &compressed_data_slice[4..];
        match header_from_bytes(compressed_data_slice)? {
            (_, CompressionType::ZSTD, ArchiveType::TAR) => Vfs::from_tar_zstd_bytes(data_bytes),
            (_, CompressionType::NONE, ArchiveType::TAR) => Vfs::from_tar_bytes(data_bytes),
        }
    }

    /// Create a vfs from raw bytes in tar format
    pub fn from_tar_bytes<Reader: Read>(tar_bytes: Reader) -> Result<Self, failure::Error> {
        let mut ar = tar::Archive::new(tar_bytes);
        init_env();
        let mut repo = RepoOpener::new()
            .create(true)
            .open("mem://wasmer_fs", "")
            .unwrap();
        for entry in ar.entries()? {
            let mut entry = entry?;
            let path = convert_to_absolute_path(entry.path().unwrap());
            let mut file = OpenOptions::new().create(true).open(&mut repo, path)?;
            io::copy(&mut entry, &mut file)?;
            file.finish().unwrap();
        }
        let vfs = Vfs {
            repo,
            fd_map: BTreeMap::new(),
        };
        Ok(vfs)
    }

    /// like read(2), will read the data for the file descriptor
    pub fn read_file(&mut self, fd: Fd, buf: &mut [u8]) -> Result<usize, failure::Error> {
        self.fd_map
            .get_mut(&fd)
            .ok_or(VfsError::FileDescriptorNotExist)?
            .read(buf)
            .map_err(|e| e.into())
    }

    /// like open(2), creates a file descriptor for the path if it exists
    pub fn open_file<P: AsRef<Path>>(&mut self, path: P) -> Result<Fd, failure::Error> {
        let mut repo = &mut self.repo;
        let path = convert_to_absolute_path(path);
        let file = OpenOptions::new().open(&mut repo, path)?;
        let fd = if self.fd_map.len() == 0 {
            0
        } else {
            let fd = *match self.fd_map.keys().max() {
                Some(fd) => fd,
                None => return Err(VfsError::CouldNotGetNextLowestFileDescriptor.into()),
            };
            fd + 1
        };
        self.fd_map.insert(fd, file);
        Ok(fd)
    }
}

#[derive(Debug, Fail)]
pub enum VfsError {
    #[fail(display = "File descriptor does not exist.")]
    FileDescriptorNotExist,
    #[fail(display = "Error when trying to read maximum file descriptor.")]
    CouldNotGetNextLowestFileDescriptor,
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
    fn open_files() {
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
    }
}
