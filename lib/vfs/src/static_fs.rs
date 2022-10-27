use anyhow::anyhow;

use std::convert::TryInto;
use std::io::{Error as IoError, ErrorKind as IoErrorKind, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use crate::mem_fs::FileSystem as MemFileSystem;
use crate::{
    FileDescriptor, FileOpener, FileSystem, FsError, Metadata, OpenOptions, OpenOptionsConfig,
    ReadDir, VirtualFile,
};
use webc::{FsEntry, FsEntryType, OwnedFsEntryFile};

/// Custom file system wrapper to map requested file paths
#[derive(Debug)]
pub struct StaticFileSystem {
    pub package: String,
    pub volumes: Arc<webc::IndexMap<String, webc::Volume<'static>>>,
    pub memory: Arc<MemFileSystem>,
}

impl StaticFileSystem {
    pub fn init(bytes: &'static [u8], package: &str) -> Option<Self> {
        let volumes = Arc::new(webc::WebC::parse_volumes_from_fileblock(bytes).ok()?);
        let fs = Self {
            package: package.to_string(),
            volumes: volumes.clone(),
            memory: Arc::new(MemFileSystem::default()),
        };
        let volume_names = fs.volumes.keys().cloned().collect::<Vec<_>>();
        for volume_name in volume_names {
            let directories = volumes.get(&volume_name).unwrap().list_directories();
            for directory in directories {
                let _ = fs.create_dir(Path::new(&directory));
            }
        }
        Some(fs)
    }
}

/// Custom file opener, returns a WebCFile
#[derive(Debug)]
struct WebCFileOpener {
    pub package: String,
    pub volumes: Arc<webc::IndexMap<String, webc::Volume<'static>>>,
    pub memory: Arc<MemFileSystem>,
}

impl FileOpener for WebCFileOpener {
    fn open(
        &mut self,
        path: &Path,
        _conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync>, FsError> {
        match get_volume_name_opt(path) {
            Some(volume) => {
                let file = (*self.volumes)
                    .get(&volume)
                    .ok_or(FsError::EntityNotFound)?
                    .get_file_entry(&format!("{}", path.display()))
                    .map_err(|_e| FsError::EntityNotFound)?;

                Ok(Box::new(WebCFile {
                    package: self.package.clone(),
                    volume,
                    volumes: self.volumes.clone(),
                    path: path.to_path_buf(),
                    entry: file,
                    cursor: 0,
                }))
            }
            None => {
                for (volume, v) in self.volumes.iter() {
                    let entry = match v.get_file_entry(&format!("{}", path.display())) {
                        Ok(s) => s,
                        Err(_) => continue, // error
                    };

                    return Ok(Box::new(WebCFile {
                        package: self.package.clone(),
                        volume: volume.clone(),
                        volumes: self.volumes.clone(),
                        path: path.to_path_buf(),
                        entry,
                        cursor: 0,
                    }));
                }
                self.memory.new_open_options().open(path)
            }
        }
    }
}

#[derive(Debug)]
pub struct WebCFile {
    pub volumes: Arc<webc::IndexMap<String, webc::Volume<'static>>>,
    pub package: String,
    pub volume: String,
    pub path: PathBuf,
    pub entry: OwnedFsEntryFile,
    pub cursor: u64,
}

impl VirtualFile for WebCFile {
    fn last_accessed(&self) -> u64 {
        0
    }
    fn last_modified(&self) -> u64 {
        0
    }
    fn created_time(&self) -> u64 {
        0
    }
    fn size(&self) -> u64 {
        self.entry.get_len()
    }
    fn set_len(&mut self, _new_size: u64) -> Result<(), FsError> {
        Ok(())
    }
    fn unlink(&mut self) -> Result<(), FsError> {
        Ok(())
    }
    fn bytes_available(&self) -> Result<usize, FsError> {
        Ok(self.size().try_into().unwrap_or(u32::MAX as usize))
    }
    fn sync_to_disk(&self) -> Result<(), FsError> {
        Ok(())
    }
    fn get_fd(&self) -> Option<FileDescriptor> {
        None
    }
}

impl Read for WebCFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        let bytes = self
            .volumes
            .get(&self.volume)
            .ok_or_else(|| {
                IoError::new(
                    IoErrorKind::NotFound,
                    anyhow!("Unknown volume {:?}", self.volume),
                )
            })?
            .get_file_bytes(&self.entry)
            .map_err(|e| IoError::new(IoErrorKind::NotFound, e))?;

        let cursor: usize = self.cursor.try_into().unwrap_or(u32::MAX as usize);
        let _start = cursor.min(bytes.len());
        let bytes = &bytes[cursor..];

        let mut len = 0;
        for (source, target) in bytes.iter().zip(buf.iter_mut()) {
            *target = *source;
            len += 1;
        }

        Ok(len)
    }
}

// WebC file is not writable, the FileOpener will return a MemoryFile for writing instead
// This code should never be executed (since writes are redirected to memory instead).
impl Write for WebCFile {
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> Result<(), IoError> {
        Ok(())
    }
}

impl Seek for WebCFile {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, IoError> {
        let self_size = self.size();
        match pos {
            SeekFrom::Start(s) => {
                self.cursor = s.min(self_size);
            }
            SeekFrom::End(e) => {
                let self_size_i64 = self_size.try_into().unwrap_or(i64::MAX);
                self.cursor = ((self_size_i64).saturating_add(e))
                    .min(self_size_i64)
                    .try_into()
                    .unwrap_or(i64::MAX as u64);
            }
            SeekFrom::Current(c) => {
                self.cursor = (self
                    .cursor
                    .saturating_add(c.try_into().unwrap_or(i64::MAX as u64)))
                .min(self_size);
            }
        }
        Ok(self.cursor)
    }
}

fn get_volume_name_opt<P: AsRef<Path>>(path: P) -> Option<String> {
    use std::path::Component::Normal;
    if let Some(Normal(n)) = path.as_ref().components().next() {
        if let Some(s) = n.to_str() {
            if s.ends_with(':') {
                return Some(s.replace(':', ""));
            }
        }
    }
    None
}

fn transform_into_read_dir<'a>(path: &Path, fs_entries: &[FsEntry<'a>]) -> crate::ReadDir {
    let entries = fs_entries
        .iter()
        .map(|e| crate::DirEntry {
            path: path.join(&*e.text),
            metadata: Ok(crate::Metadata {
                ft: translate_file_type(e.fs_type),
                accessed: 0,
                created: 0,
                modified: 0,
                len: e.get_len(),
            }),
        })
        .collect();

    crate::ReadDir::new(entries)
}

impl FileSystem for StaticFileSystem {
    fn read_dir(&self, path: &Path) -> Result<ReadDir, FsError> {
        let path = normalizes_path(path);
        for volume in self.volumes.values() {
            let read_dir_result = volume
                .read_dir(&path)
                .map(|o| transform_into_read_dir(Path::new(&path), o.as_ref()))
                .map_err(|_| FsError::EntityNotFound);

            match read_dir_result {
                Ok(o) => {
                    return Ok(o);
                }
                Err(_) => {
                    continue;
                }
            }
        }

        self.memory.read_dir(Path::new(&path))
    }
    fn create_dir(&self, path: &Path) -> Result<(), FsError> {
        let path = normalizes_path(path);
        let result = self.memory.create_dir(Path::new(&path));
        result
    }
    fn remove_dir(&self, path: &Path) -> Result<(), FsError> {
        let path = normalizes_path(path);
        let result = self.memory.remove_dir(Path::new(&path));
        if self
            .volumes
            .values()
            .find_map(|v| v.get_file_entry(&path).ok())
            .is_some()
        {
            Ok(())
        } else {
            result
        }
    }
    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        let from = normalizes_path(from);
        let to = normalizes_path(to);
        let result = self.memory.rename(Path::new(&from), Path::new(&to));
        if self
            .volumes
            .values()
            .find_map(|v| v.get_file_entry(&from).ok())
            .is_some()
        {
            Ok(())
        } else {
            result
        }
    }
    fn metadata(&self, path: &Path) -> Result<Metadata, FsError> {
        let path = normalizes_path(path);
        if let Some(fs_entry) = self
            .volumes
            .values()
            .find_map(|v| v.get_file_entry(&path).ok())
        {
            Ok(Metadata {
                ft: translate_file_type(FsEntryType::File),
                accessed: 0,
                created: 0,
                modified: 0,
                len: fs_entry.get_len(),
            })
        } else if let Some(_fs) = self.volumes.values().find_map(|v| v.read_dir(&path).ok()) {
            Ok(Metadata {
                ft: translate_file_type(FsEntryType::Dir),
                accessed: 0,
                created: 0,
                modified: 0,
                len: 0,
            })
        } else {
            self.memory.metadata(Path::new(&path))
        }
    }
    fn remove_file(&self, path: &Path) -> Result<(), FsError> {
        let path = normalizes_path(path);
        let result = self.memory.remove_file(Path::new(&path));
        if self
            .volumes
            .values()
            .find_map(|v| v.get_file_entry(&path).ok())
            .is_some()
        {
            Ok(())
        } else {
            result
        }
    }
    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(Box::new(WebCFileOpener {
            package: self.package.clone(),
            volumes: self.volumes.clone(),
            memory: self.memory.clone(),
        }))
    }
    fn symlink_metadata(&self, path: &Path) -> Result<Metadata, FsError> {
        let path = normalizes_path(path);
        if let Some(fs_entry) = self
            .volumes
            .values()
            .find_map(|v| v.get_file_entry(&path).ok())
        {
            Ok(Metadata {
                ft: translate_file_type(FsEntryType::File),
                accessed: 0,
                created: 0,
                modified: 0,
                len: fs_entry.get_len(),
            })
        } else if self
            .volumes
            .values()
            .find_map(|v| v.read_dir(&path).ok())
            .is_some()
        {
            Ok(Metadata {
                ft: translate_file_type(FsEntryType::Dir),
                accessed: 0,
                created: 0,
                modified: 0,
                len: 0,
            })
        } else {
            self.memory.symlink_metadata(Path::new(&path))
        }
    }
}

fn normalizes_path(path: &Path) -> String {
    let path = format!("{}", path.display());
    if !path.starts_with('/') {
        format!("/{path}")
    } else {
        path
    }
}

fn translate_file_type(f: FsEntryType) -> crate::FileType {
    crate::FileType {
        dir: f == FsEntryType::Dir,
        file: f == FsEntryType::File,
        symlink: false,
        char_device: false,
        block_device: false,
        socket: false,
        fifo: false,
    }
}
