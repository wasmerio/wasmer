use anyhow::anyhow;
use futures::future::BoxFuture;
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};

use std::convert::TryInto;
use std::io::{self, Error as IoError, ErrorKind as IoErrorKind, SeekFrom};
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::mem_fs::FileSystem as MemFileSystem;
use crate::{
    FileOpener, FileSystem, FsError, Metadata, OpenOptions, OpenOptionsConfig, ReadDir, VirtualFile,
};
use indexmap::IndexMap;
use webc::v1::{FsEntry, FsEntryType, OwnedFsEntryFile};

/// Custom file system wrapper to map requested file paths
#[derive(Debug)]
pub struct StaticFileSystem {
    pub package: String,
    pub volumes: Arc<IndexMap<String, webc::v1::Volume<'static>>>,
    pub memory: Arc<MemFileSystem>,
}

impl StaticFileSystem {
    pub fn init(bytes: &'static [u8], package: &str) -> Option<Self> {
        let volumes = Arc::new(webc::v1::WebC::parse_volumes_from_fileblock(bytes).ok()?);
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
impl FileOpener for StaticFileSystem {
    fn open(
        &self,
        path: &Path,
        _conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync>, FsError> {
        match get_volume_name_opt(path) {
            Some(volume) => {
                let file = (*self.volumes)
                    .get(&volume)
                    .ok_or(FsError::EntryNotFound)?
                    .get_file_entry(path.to_string_lossy().as_ref())
                    .map_err(|_e| FsError::EntryNotFound)?;

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
                    let entry = match v.get_file_entry(path.to_string_lossy().as_ref()) {
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
    pub volumes: Arc<IndexMap<String, webc::v1::Volume<'static>>>,
    pub package: String,
    pub volume: String,
    pub path: PathBuf,
    pub entry: OwnedFsEntryFile,
    pub cursor: u64,
}

#[async_trait::async_trait]
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
    fn set_len(&mut self, _new_size: u64) -> crate::Result<()> {
        Ok(())
    }
    fn unlink(&mut self) -> Result<(), FsError> {
        Ok(())
    }
    fn poll_read_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let remaining = self.entry.get_len() - self.cursor;
        Poll::Ready(Ok(remaining as usize))
    }
    fn poll_write_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(0))
    }
}

impl AsyncRead for WebCFile {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
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

        if bytes.len() > buf.remaining() {
            let remaining = buf.remaining();
            buf.put_slice(&bytes[..remaining]);
        } else {
            buf.put_slice(bytes);
        }
        Poll::Ready(Ok(()))
    }
}

// WebC file is not writable, the FileOpener will return a MemoryFile for writing instead
// This code should never be executed (since writes are redirected to memory instead).
impl AsyncWrite for WebCFile {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncSeek for WebCFile {
    fn start_seek(mut self: Pin<&mut Self>, pos: io::SeekFrom) -> io::Result<()> {
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
        Ok(())
    }
    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(self.cursor))
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

fn transform_into_read_dir(path: &Path, fs_entries: &[FsEntry<'_>]) -> crate::ReadDir {
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
    fn readlink(&self, path: &Path) -> crate::Result<PathBuf> {
        let path = normalizes_path(path);
        if self
            .volumes
            .values()
            .find_map(|v| v.get_file_entry(&path).ok())
            .is_some()
        {
            Err(FsError::InvalidInput)
        } else {
            self.memory.readlink(Path::new(&path))
        }
    }

    fn read_dir(&self, path: &Path) -> Result<ReadDir, FsError> {
        let path = normalizes_path(path);
        for volume in self.volumes.values() {
            let read_dir_result = volume
                .read_dir(&path)
                .map(|o| transform_into_read_dir(Path::new(&path), o.as_ref()))
                .map_err(|_| FsError::EntryNotFound);

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
    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<(), FsError>> {
        Box::pin(async {
            let from = normalizes_path(from);
            let to = normalizes_path(to);
            let result = self.memory.rename(Path::new(&from), Path::new(&to)).await;
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
        })
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
        OpenOptions::new(self)
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

    fn mount(
        &self,
        _name: String,
        _path: &Path,
        _fs: Box<dyn FileSystem + Send + Sync>,
    ) -> Result<(), FsError> {
        Err(FsError::Unsupported)
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
