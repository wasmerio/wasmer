use std::{
    convert::{TryFrom, TryInto},
    io::{self, Error as IoError, ErrorKind as IoErrorKind, SeekFrom},
    ops::Deref,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use anyhow::anyhow;
use futures::future::BoxFuture;
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};
use webc::v1::{FsEntry, FsEntryType, OwnedFsEntryFile, WebC};

use crate::{
    mem_fs::FileSystem as MemFileSystem, FileOpener, FileSystem, FsError, Metadata, OpenOptions,
    OpenOptionsConfig, ReadDir, VirtualFile,
};

/// Custom file system wrapper to map requested file paths
#[derive(Debug)]
pub struct WebcFileSystem<T>
where
    T: std::fmt::Debug + Send + Sync + 'static,
{
    pub webc: Arc<T>,
    pub memory: Arc<MemFileSystem>,
    top_level_dirs: Vec<String>,
    volumes: Vec<webc::v1::Volume<'static>>,
}

impl<T> WebcFileSystem<T>
where
    T: std::fmt::Debug + Send + Sync + 'static,
    T: Deref<Target = WebC<'static>>,
{
    pub fn init(webc: Arc<T>, package: &str) -> Self {
        let mut fs = Self {
            webc: webc.clone(),
            memory: Arc::new(MemFileSystem::default()),
            top_level_dirs: Vec::new(),
            volumes: Vec::new(),
        };

        for volume in webc.get_volumes_for_package(package) {
            if let Some(vol_ref) = webc.volumes.get(&volume) {
                fs.volumes.push(vol_ref.clone());
            }
            for directory in webc.list_directories(&volume) {
                fs.top_level_dirs.push(directory.clone());
                let _ = fs.create_dir(Path::new(&directory));
            }
        }
        fs
    }

    pub fn init_all(webc: Arc<T>) -> Self {
        let mut fs = Self {
            webc: webc.clone(),
            memory: Arc::new(MemFileSystem::default()),
            top_level_dirs: Vec::new(),
            volumes: webc.volumes.clone().into_values().collect(),
        };
        for (header, _) in webc.volumes.iter() {
            for directory in webc.list_directories(header) {
                fs.top_level_dirs.push(directory.clone());
                let _ = fs.create_dir(Path::new(&directory));
            }
        }
        fs
    }

    pub fn top_level_dirs(&self) -> &Vec<String> {
        &self.top_level_dirs
    }
}

/// Custom file opener, returns a WebCFile
impl<T> FileOpener for WebcFileSystem<T>
where
    T: std::fmt::Debug + Send + Sync + 'static,
    T: Deref<Target = WebC<'static>>,
{
    fn open(
        &self,
        path: &Path,
        _conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync>, FsError> {
        match get_volume_name_opt(path) {
            Some(volume) => {
                let file = self
                    .webc
                    .volumes
                    .get(&volume)
                    .ok_or(FsError::EntryNotFound)?
                    .get_file_entry(path.to_string_lossy().as_ref())
                    .map_err(|_e| FsError::EntryNotFound)?;

                Ok(Box::new(WebCFile {
                    volume,
                    webc: self.webc.clone(),
                    path: path.to_path_buf(),
                    entry: file,
                    cursor: 0,
                }))
            }
            None => {
                for (volume, _) in self.webc.volumes.iter() {
                    let v = match self.webc.volumes.get(volume) {
                        Some(s) => s,
                        None => continue, // error
                    };

                    let entry = match v.get_file_entry(path.to_string_lossy().as_ref()) {
                        Ok(s) => s,
                        Err(_) => continue, // error
                    };

                    return Ok(Box::new(WebCFile {
                        volume: volume.clone(),
                        webc: self.webc.clone(),
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
struct WebCFile<T>
where
    T: std::fmt::Debug + Send + Sync + 'static,
{
    pub webc: Arc<T>,
    pub volume: String,
    #[allow(dead_code)]
    pub path: PathBuf,
    pub entry: OwnedFsEntryFile,
    pub cursor: u64,
}

impl<T> VirtualFile for WebCFile<T>
where
    T: std::fmt::Debug + Send + Sync + 'static,
    T: Deref<Target = WebC<'static>>,
{
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

impl<T> AsyncRead for WebCFile<T>
where
    T: std::fmt::Debug + Send + Sync + 'static,
    T: Deref<Target = WebC<'static>>,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let bytes = self
            .webc
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

        let start: usize = self.cursor.try_into().unwrap();
        let remaining = &bytes[start..];
        let bytes_read = remaining.len().min(buf.remaining());
        let bytes = &remaining[..bytes_read];

        buf.put_slice(bytes);
        self.cursor += u64::try_from(bytes_read).unwrap();

        Poll::Ready(Ok(()))
    }
}

// WebC file is not writable, the FileOpener will return a MemoryFile for writing instead
// This code should never be executed (since writes are redirected to memory instead).
impl<T> AsyncWrite for WebCFile<T>
where
    T: std::fmt::Debug + Send + Sync + 'static,
{
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

impl<T> AsyncSeek for WebCFile<T>
where
    T: std::fmt::Debug + Send + Sync + 'static,
    T: Deref<Target = WebC<'static>>,
{
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

#[allow(dead_code)]
fn get_volume_name<P: AsRef<Path>>(path: P) -> String {
    get_volume_name_opt(path).unwrap_or_else(|| "atom".to_string())
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

impl<T> FileSystem for WebcFileSystem<T>
where
    T: std::fmt::Debug + Send + Sync + 'static,
    T: Deref<Target = WebC<'static>>,
{
    fn read_dir(&self, path: &Path) -> Result<ReadDir, FsError> {
        let path = normalizes_path(path);
        let read_dir_result = self
            .volumes
            .iter()
            .filter_map(|v| v.read_dir(&path).ok())
            .next()
            .map(|o| transform_into_read_dir(Path::new(&path), o.as_ref()))
            .ok_or(FsError::EntryNotFound);

        match read_dir_result {
            Ok(o) => Ok(o),
            Err(_) => self.memory.read_dir(Path::new(&path)),
        }
    }
    fn create_dir(&self, path: &Path) -> Result<(), FsError> {
        let path = normalizes_path(path);
        let result = self.memory.create_dir(Path::new(&path));
        result
    }
    fn remove_dir(&self, path: &Path) -> Result<(), FsError> {
        let path = normalizes_path(path);
        let result = self.memory.remove_dir(Path::new(&path));
        if self.volumes.iter().any(|v| v.get_file_entry(&path).is_ok()) {
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
            if self.volumes.iter().any(|v| v.get_file_entry(&from).is_ok()) {
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
            .iter()
            .filter_map(|v| v.get_file_entry(&path).ok())
            .next()
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
            .iter()
            .filter_map(|v| v.read_dir(&path).ok())
            .next()
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
            self.memory.metadata(Path::new(&path))
        }
    }
    fn remove_file(&self, path: &Path) -> Result<(), FsError> {
        let path = normalizes_path(path);
        let result = self.memory.remove_file(Path::new(&path));
        if self
            .volumes
            .iter()
            .filter_map(|v| v.get_file_entry(&path).ok())
            .next()
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
            .iter()
            .filter_map(|v| v.get_file_entry(&path).ok())
            .next()
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
            .iter()
            .filter_map(|v| v.read_dir(&path).ok())
            .next()
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

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use tokio::io::AsyncReadExt;
    use webc::v1::{ParseOptions, WebCOwned};

    use super::*;

    #[tokio::test]
    async fn read_a_file_from_the_webc_fs() {
        let webc: &[u8] = include_bytes!("../../c-api/examples/assets/python-0.1.0.wasmer");
        let options = ParseOptions::default();
        let webc = WebCOwned::parse(Bytes::from_static(webc), &options).unwrap();

        let fs = WebcFileSystem::init_all(Arc::new(webc));

        let mut f = fs
            .new_open_options()
            .read(true)
            .open(Path::new("/lib/python3.6/collections/abc.py"))
            .unwrap();

        let mut abc_py = String::new();
        f.read_to_string(&mut abc_py).await.unwrap();
        assert_eq!(
            abc_py,
            "from _collections_abc import *\nfrom _collections_abc import __all__\n"
        );
    }
}
