use crate::vfs::file_like::FileLike;
use crate::vfs::vfs_header::{header_from_bytes, ArchiveType, CompressionType};
use crate::vfs::virtual_file::VirtualFile;
use hashbrown::HashMap;
use std::cell::RefCell;
use std::io;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use tar::EntryType;
use zbox::{init_env, OpenOptions, Repo, RepoOpener};

pub struct Vfs {
    repo: Repo,
    device_files: HashMap<PathBuf, Rc<RefCell<dyn FileLike>>>,
}

impl Vfs {
    /// Like `VfsBacking::from_tar_bytes` except it also decompresses from the zstd format.
    pub fn from_tar_zstd_bytes<Reader: Read>(tar_bytes: Reader) -> Result<Self, failure::Error> {
        let result = zstd::decode_all(tar_bytes);
        let decompressed_data = result.unwrap();
        Self::from_tar_bytes(&decompressed_data[..])
    }

    /// Match on the type of the compressed-archive and select the correct unpack method
    pub fn from_compressed_bytes(compressed_data_slice: &[u8]) -> Result<Self, failure::Error> {
        let data_bytes = &compressed_data_slice[4..];
        match header_from_bytes(compressed_data_slice)? {
            (_, CompressionType::ZSTD, ArchiveType::TAR) => Self::from_tar_zstd_bytes(data_bytes),
            (_, CompressionType::NONE, ArchiveType::TAR) => Self::from_tar_bytes(data_bytes),
        }
    }

    /// Create a vfs from raw bytes in tar format
    pub fn from_tar_bytes<Reader: Read>(tar_bytes: Reader) -> Result<Self, failure::Error> {
        init_env();
        let mut repo = RepoOpener::new()
            .create(true)
            .open("mem://wasmer_fs", "")
            .unwrap();
        let _errors = tar::Archive::new(tar_bytes)
            .entries()?
            .map(|entry| {
                let mut entry: tar::Entry<Reader> = entry?;
                let path = entry.path()?;
                let path = convert_to_absolute_path(path);
                let _result = match (entry.header().entry_type(), path.parent()) {
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

        //        let import_errors = errors.iter().filter_map(|e| e.err()).collect::<Vec<_>>();

        let vfs = Self {
            repo,
            device_files: HashMap::new(),
            //            import_errors: vec![],
        };
        Ok(vfs)
    }

    pub fn new() -> Result<(Self, Vec<VfsError>), failure::Error> {
        init_env();
        let repo = RepoOpener::new()
            .create(true)
            .open("mem://wasmer_fs", "")
            .unwrap();
        Ok((
            Vfs {
                repo,
                device_files: HashMap::new(),
            },
            vec![],
        ))
    }

    pub fn open_file<P: AsRef<Path>>(&mut self, path: P) -> Option<Rc<RefCell<dyn FileLike>>> {
        init_env();
        let path = convert_to_absolute_path(path);
        if let Ok(file) = OpenOptions::new().write(true).open(&mut self.repo, &path) {
            Some(Rc::new(RefCell::new(VirtualFile::new(file))))
        } else if let Some(dev_file) = self.device_files.get(&path) {
            Some(dev_file.clone())
        } else {
            None
        }
    }

    pub fn make_dir<P: AsRef<Path>>(&mut self, path: P) {
        self.repo.create_dir_all(path).unwrap();
    }

    pub fn create_device_file<P: AsRef<Path>>(&mut self, path: P, file: Rc<RefCell<dyn FileLike>>) {
        self.device_files.insert(path.as_ref().to_path_buf(), file);
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

pub type Handle = i32;
#[derive(Debug, Fail)]
pub enum VfsError {
    #[fail(display = "File with file descriptor \"{}\" does not exist.", _0)]
    FileWithFileDescriptorNotExist(Handle),
    #[fail(display = "File descriptor does not exist.")]
    FileDescriptorNotExist(Handle),
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
