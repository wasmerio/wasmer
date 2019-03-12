use hashbrown::HashMap;
use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;

/// Simply an alias. May become a more complicated type in the future.
pub type Fd = isize;

/// Index into the file data vec.
pub type DataIndex = usize;

/// A simple key representing a path or a file descriptor. This filesystem treats paths and file
/// descriptor as first class citizens. A key has access to an index in the filesystem data.
#[derive(Hash, Eq, PartialEq, Debug)]
pub enum DataKey {
    Path(PathBuf),
    Fd(Fd),
}

pub struct VfsBacking {
    /// The file data
    blocks: Vec<Vec<u8>>,
    /// Map of file descriptors or paths to indexes in the file data
    data: HashMap<DataKey, DataIndex>,
    /// Counter for file descriptors
    fd_count: Arc<AtomicIsize>,
}

impl VfsBacking {
    /// like read(2), will read the data for the file descriptor
    pub fn read_file<Writer: Write>(
        &mut self,
        fd: Fd,
        mut buf: Writer,
    ) -> Result<usize, failure::Error> {
        let key = DataKey::Fd(fd);
        let data_index = *self
            .data
            .get(&key)
            .ok_or(VfsBackingError::FileDescriptorNotExist)?;
        let data = self
            .blocks
            .get(data_index)
            .ok_or(VfsBackingError::DataDoesNotExist)?;
        buf.write(&data[..])
            .map_err(|_| VfsBackingError::CopyError.into())
            .map(|s| s as _)
    }

    /// like open(2), creates a file descriptor for the path if it exists
    pub fn open_file<P: AsRef<Path>>(&mut self, path: P) -> Result<Fd, failure::Error> {
        let path = path.as_ref().to_path_buf();
        let key = DataKey::Path(path);
        let data_index = *self
            .data
            .get(&key)
            .ok_or(VfsBackingError::PathDoesNotExist)?;
        // create an insert a file descriptor key
        let fd = self.fd_count.fetch_add(1, Ordering::SeqCst);
        let fd_key = DataKey::Fd(fd);
        let _ = self.data.insert(fd_key, data_index);
        Ok(fd)
    }

    /// Like `VfsBacking::from_tar_bytes` except it also decompresses from the zstd format.
    pub fn from_tar_zstd_bytes<Reader: Read>(tar_bytes: Reader) -> Result<Self, failure::Error> {
        let result = zstd::decode_all(tar_bytes);
        let decompressed_data = result.unwrap();
        VfsBacking::from_tar_bytes(&decompressed_data[..])
    }

    /// Create a vfs from raw bytes in tar format
    pub fn from_tar_bytes<Reader: Read>(tar_bytes: Reader) -> Result<Self, failure::Error> {
        let mut ar = tar::Archive::new(tar_bytes);
        let mut data = HashMap::new();
        let mut blocks = vec![];
        for entry in ar.entries()? {
            let mut entry = entry?;
            // make a key from a path and insert the index of the
            let path = entry.path().unwrap().to_path_buf();
            let key = DataKey::Path(path);
            let index = blocks.len();
            data.insert(key, index);
            // read the entry into a buffer and then push it into the file store
            let mut file_data: Vec<u8> = vec![];
            entry.read_to_end(&mut file_data).unwrap();
            blocks.push(file_data);
        }
        let vfs = VfsBacking {
            blocks,
            data,
            fd_count: Arc::new(AtomicIsize::new(0)),
        };
        Ok(vfs)
    }
}

#[derive(Debug, Fail)]
pub enum VfsBackingError {
    #[fail(display = "Data does not exist.")]
    DataDoesNotExist,
    #[fail(display = "Path does not exist.")]
    PathDoesNotExist,
    #[fail(display = "File descriptor does not exist.")]
    FileDescriptorNotExist,
    #[fail(display = "Error while copying to buffer")]
    CopyError,
}

#[derive(Debug, Fail)]
pub enum VfsError {
    #[fail(display = "File does not exist.")]
    FileDoesNotExist,
}

#[cfg(test)]
mod open_test {
    use crate::vfs::vfs::VfsBacking;
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
        let vfs_result = VfsBacking::from_tar_bytes(&archive[..]);
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
        let vfs_result = VfsBacking::from_tar_bytes(&archive[..]);
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
    use crate::vfs::vfs::VfsBacking;
    use std::fs::File;
    use std::io::Write;
    use tempdir;

    #[test]
    fn empty_archive() {
        // SETUP: create temp dir and files
        let empty_archive = vec![];
        // SETUP: create virtual filesystem with tar data
        let vfs_result = VfsBacking::from_tar_bytes(&empty_archive[..]);
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
        let vfs_result = VfsBacking::from_tar_bytes(&archive[..]);
        // ASSERT:
        assert!(
            vfs_result.is_ok(),
            "Failed to create file system from archive"
        );
        let mut vfs = vfs_result.unwrap();
        // read the file
        let fd = vfs.open_file("foo.txt").unwrap();
        let mut actual_data: Vec<u8> = Vec::new();
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
        let vfs_result = VfsBacking::from_tar_bytes(&archive[..]);
        // ASSERT:
        assert!(
            vfs_result.is_ok(),
            "Failed to create file system from archive"
        );
        let mut vfs = vfs_result.unwrap();
        // read the file
        let foo_fd = vfs.open_file("foo.txt").unwrap();
        let bar_fd = vfs.open_file("bar.txt").unwrap();
        let mut foo_actual_data: Vec<u8> = Vec::new();
        let foo_read_result = vfs.read_file(foo_fd, &mut foo_actual_data);
        let mut bar_actual_data: Vec<u8> = Vec::new();
        let bar_read_result = vfs.read_file(bar_fd, &mut bar_actual_data);
        assert!(foo_read_result.is_ok(), "Failed to read foo.txt from vfs");
        assert!(bar_read_result.is_ok(), "Failed to read bar.txt from vfs");
        let foo_expected_data = Vec::from("foo foo foo\n");
        let bar_expected_data = Vec::from("bar bar\n");
        assert_eq!(
            foo_actual_data, foo_expected_data,
            "Contents of `foo.txt` is not correct"
        );
        assert_eq!(
            bar_actual_data, bar_expected_data,
            "Contents of `bar.txt` is not correct"
        );
    }
}
