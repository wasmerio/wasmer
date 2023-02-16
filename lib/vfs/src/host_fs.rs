use crate::{
    DirEntry, FileDescriptor, FileType, FsError, Metadata, OpenOptions, OpenOptionsConfig, ReadDir,
    Result, VirtualFile,
};
#[cfg(feature = "enable-serde")]
use serde::{de, Deserialize, Serialize};
use std::collections::BTreeMap;
use std::convert::TryInto;
use std::fs;
use std::io::{self, Read, Seek, Write};
#[cfg(unix)]
use std::os::unix::io::{AsRawFd, RawFd};
#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

trait TryIntoFileDescriptor {
    type Error;

    fn try_into_filedescriptor(&self) -> std::result::Result<FileDescriptor, Self::Error>;
}

#[cfg(unix)]
impl<T> TryIntoFileDescriptor for T
where
    T: AsRawFd,
{
    type Error = FsError;

    fn try_into_filedescriptor(&self) -> std::result::Result<FileDescriptor, Self::Error> {
        Ok(FileDescriptor(
            self.as_raw_fd()
                .try_into()
                .map_err(|_| FsError::InvalidFd)?,
        ))
    }
}

#[cfg(unix)]
impl TryInto<RawFd> for FileDescriptor {
    type Error = FsError;

    fn try_into(self) -> std::result::Result<RawFd, Self::Error> {
        self.0.try_into().map_err(|_| FsError::InvalidFd)
    }
}

#[cfg(windows)]
impl<T> TryIntoFileDescriptor for T
where
    T: AsRawHandle,
{
    type Error = FsError;

    fn try_into_filedescriptor(&self) -> std::result::Result<FileDescriptor, Self::Error> {
        Ok(FileDescriptor(self.as_raw_handle() as usize))
    }
}

#[cfg(windows)]
impl TryInto<RawHandle> for FileDescriptor {
    type Error = FsError;

    fn try_into(self) -> std::result::Result<RawHandle, Self::Error> {
        Ok(self.0 as RawHandle)
    }
}

#[derive(Debug, Clone)]
pub struct HostFileSystemDescriptor {
    pub path: PathBuf,
    pub alias: Option<String>,
    pub read: bool,
    pub write: bool,
    pub create: bool,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct FileSystem {
    /// Host-mapped file system paths
    pub mapped_dirs: BTreeMap<HostFsAlias, PathBuf>,
}

#[derive(Debug, Ord, Eq, PartialOrd, PartialEq, Clone)]
pub enum HostFsAlias {
    Root,
    Path(String),
}

impl Default for FileSystem {
    fn default() -> FileSystem {
        FileSystem {
            // mapped_dirs: vec![(HostFsAlias::Root, std::env::current_dir().unwrap())]
            mapped_dirs: vec![(HostFsAlias::Root, Path::new("/").to_path_buf())]
                .into_iter()
                .collect(),
        }
    }
}

impl FileSystem {
    pub fn new(preopens: &[HostFileSystemDescriptor]) -> Self {
        Self {
            mapped_dirs: preopens
                .iter()
                .filter_map(|h| {
                    // TODO: incorporate file system rights from preopens
                    let alias = h.alias.clone()?;
                    let converted_alias;
                    if alias == "." || alias == "/" {
                        converted_alias = HostFsAlias::Root;
                    } else if alias.starts_with("./") {
                        converted_alias = HostFsAlias::Path(alias.strip_prefix("./")?.to_string());
                    } else if alias.starts_with('/') {
                        converted_alias = HostFsAlias::Path(alias.strip_prefix('/')?.to_string());
                    } else {
                        converted_alias = HostFsAlias::Path(alias);
                    }
                    let alias = converted_alias;
                    let path = h.path.clone();
                    Some((alias, path))
                })
                .collect(),
        }
    }
}

fn get_normalized_host_path(
    requested_path: &Path,
    mapped_dirs: &BTreeMap<HostFsAlias, PathBuf>,
    expect_file_to_exist_already: bool,
) -> Option<PathBuf> {
    for (alias, mapped_path) in mapped_dirs.iter() {
        let mapped_path = match mapped_path.canonicalize() {
            Ok(o) => o,
            Err(_) => continue, // TODO: wrong mapped path = error?
        };
        let requested_path = requested_path
            .strip_prefix(&mapped_path)
            .unwrap_or(requested_path);
        let requested_path = requested_path.strip_prefix("/").unwrap_or(requested_path);
        let requested_path = requested_path.strip_prefix("./").unwrap_or(requested_path);
        match alias {
            HostFsAlias::Root => {
                if !expect_file_to_exist_already
                    || fs::metadata(&mapped_path.join(requested_path)).is_ok()
                {
                    // Path is not absolute, but may not exist yet
                    // TODO: -root mapping overwhadows everything else
                    return Some(mapped_path.join(requested_path));
                }
            }
            HostFsAlias::Path(p) => {
                if requested_path.starts_with(p)
                    && fs::metadata(&mapped_path.join(requested_path)).is_ok()
                {
                    return Some(mapped_path.join(requested_path));
                }
            }
        }
    }

    None
}

#[test]
fn test_host_filesystem_sanity() {
    use crate::FileSystem;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();
    std::fs::write(path.join("hello.txt"), &[]).unwrap();
    let filesystem = crate::host_fs::FileSystem {
        mapped_dirs: vec![(HostFsAlias::Root, path.to_path_buf())]
            .into_iter()
            .collect(),
    };
    let read_dir_root = filesystem.read_dir(&Path::new("/")).unwrap();
    let files = read_dir_root
        .filter_map(|r| Some(r.ok()?.file_name().to_string_lossy().to_string()))
        .collect::<Vec<_>>();
    assert_eq!(files, vec!["hello.txt".to_string()]);
}

impl crate::FileSystem for FileSystem {
    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        let normalized_path = get_normalized_host_path(path, &self.mapped_dirs, true)
            .ok_or(FsError::EntityNotFound)?;
        let read_dir = fs::read_dir(&normalized_path)?;
        let data = read_dir
            .map(|entry| {
                let entry = entry?;
                let metadata = entry.metadata()?;
                Ok(DirEntry {
                    path: entry.path(),
                    metadata: Ok(metadata.try_into()?),
                })
            })
            .collect::<std::result::Result<Vec<DirEntry>, io::Error>>()
            .map_err::<FsError, _>(Into::into)?;
        Ok(ReadDir::new(data))
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        // Try to get the parent of the requested path and see which mapped path matches
        let mut nonexisting_dir = path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        nonexisting_dir.pop();
        let nonexisting_dir_parent = nonexisting_dir.into_iter().collect::<PathBuf>();
        let nonexisting_normalized =
            get_normalized_host_path(&nonexisting_dir_parent, &self.mapped_dirs, true)
                .ok_or(FsError::EntityNotFound)?;
        let path = nonexisting_normalized.join(path.file_name().ok_or(FsError::EntityNotFound)?);
        fs::create_dir(path).map_err(Into::into)
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        let normalized_path = get_normalized_host_path(path, &self.mapped_dirs, true)
            .ok_or(FsError::EntityNotFound)?;
        fs::remove_dir(&normalized_path).map_err(Into::into)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let from_normalized = get_normalized_host_path(from, &self.mapped_dirs, true)
            .ok_or(FsError::EntityNotFound)?;
        let mut to_components = to
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        let last_component = to_components.pop();
        let to_parent = to_components.into_iter().collect::<PathBuf>();
        // expect that the parent of the "to" path already exists
        let to_normalized = get_normalized_host_path(&to_parent, &self.mapped_dirs, true)
            .ok_or(FsError::EntityNotFound)?;
        let to_normalized_final = match last_component {
            Some(s) => to_normalized.join(s),
            None => to_normalized,
        };
        fs::rename(&from_normalized, &to_normalized_final).map_err(Into::into)
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        let normalized_path = get_normalized_host_path(path, &self.mapped_dirs, true)
            .ok_or(FsError::EntityNotFound)?;
        fs::remove_file(&normalized_path).map_err(Into::into)
    }

    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(Box::new(FileOpener {
            mapped_dirs: self.mapped_dirs.clone(),
        }))
    }

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        let normalized_path = get_normalized_host_path(path, &self.mapped_dirs, true)
            .ok_or(FsError::EntityNotFound)?;
        fs::metadata(&normalized_path)
            .and_then(TryInto::try_into)
            .map_err(Into::into)
    }
}

impl TryInto<Metadata> for fs::Metadata {
    type Error = io::Error;

    fn try_into(self) -> std::result::Result<Metadata, Self::Error> {
        let filetype = self.file_type();
        let (char_device, block_device, socket, fifo) = {
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileTypeExt;
                (
                    filetype.is_char_device(),
                    filetype.is_block_device(),
                    filetype.is_socket(),
                    filetype.is_fifo(),
                )
            }
            #[cfg(not(unix))]
            {
                (false, false, false, false)
            }
        };

        Ok(Metadata {
            ft: FileType {
                dir: filetype.is_dir(),
                file: filetype.is_file(),
                symlink: filetype.is_symlink(),
                char_device,
                block_device,
                socket,
                fifo,
            },
            accessed: self
                .accessed()
                .and_then(|time| {
                    time.duration_since(UNIX_EPOCH)
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                })
                .map_or(0, |time| time.as_nanos() as u64),
            created: self
                .created()
                .and_then(|time| {
                    time.duration_since(UNIX_EPOCH)
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                })
                .map_or(0, |time| time.as_nanos() as u64),
            modified: self
                .modified()
                .and_then(|time| {
                    time.duration_since(UNIX_EPOCH)
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                })
                .map_or(0, |time| time.as_nanos() as u64),
            len: self.len(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct FileOpener {
    /// Host-mapped file system paths
    pub mapped_dirs: BTreeMap<HostFsAlias, PathBuf>,
}

impl crate::FileOpener for FileOpener {
    fn open(
        &mut self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>> {
        let file_may_not_exist = conf.create() || conf.create_new();
        let normalized_path =
            get_normalized_host_path(path, &self.mapped_dirs, !file_may_not_exist)
                .ok_or(FsError::EntityNotFound)?;
        // TODO: handle create implying write, etc.
        let read = conf.read();
        let write = conf.write();
        let append = conf.append();
        let mut oo = fs::OpenOptions::new();
        oo.read(conf.read())
            .write(conf.write())
            .create_new(conf.create_new())
            .create(conf.create())
            .append(conf.append())
            .truncate(conf.truncate())
            .open(&normalized_path)
            .map_err(Into::into)
            .map(|file| {
                Box::new(File::new(file, path.to_owned(), read, write, append))
                    as Box<dyn VirtualFile + Send + Sync + 'static>
            })
    }
}

/// A thin wrapper around `std::fs::File`
#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize))]
pub struct File {
    #[cfg_attr(feature = "enable-serde", serde(skip_serializing))]
    pub inner: fs::File,
    pub host_path: PathBuf,
    #[cfg(feature = "enable-serde")]
    flags: u16,
}

#[cfg(feature = "enable-serde")]
impl<'de> Deserialize<'de> for File {
    fn deserialize<D>(deserializer: D) -> std::result::Result<File, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            HostPath,
            Flags,
        }

        struct FileVisitor;

        impl<'de> de::Visitor<'de> for FileVisitor {
            type Value = File;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct File")
            }

            fn visit_seq<V>(self, mut seq: V) -> std::result::Result<Self::Value, V::Error>
            where
                V: de::SeqAccess<'de>,
            {
                let host_path = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let flags = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let inner = fs::OpenOptions::new()
                    .read(flags & File::READ != 0)
                    .write(flags & File::WRITE != 0)
                    .append(flags & File::APPEND != 0)
                    .open(&host_path)
                    .map_err(|_| de::Error::custom("Could not open file on this system"))?;
                Ok(File {
                    inner,
                    host_path,
                    flags,
                })
            }

            fn visit_map<V>(self, mut map: V) -> std::result::Result<Self::Value, V::Error>
            where
                V: de::MapAccess<'de>,
            {
                let mut host_path = None;
                let mut flags = None;
                while let Some(key) = map.next_key()? {
                    match key {
                        Field::HostPath => {
                            if host_path.is_some() {
                                return Err(de::Error::duplicate_field("host_path"));
                            }
                            host_path = Some(map.next_value()?);
                        }
                        Field::Flags => {
                            if flags.is_some() {
                                return Err(de::Error::duplicate_field("flags"));
                            }
                            flags = Some(map.next_value()?);
                        }
                    }
                }
                let host_path = host_path.ok_or_else(|| de::Error::missing_field("host_path"))?;
                let flags = flags.ok_or_else(|| de::Error::missing_field("flags"))?;
                let inner = fs::OpenOptions::new()
                    .read(flags & File::READ != 0)
                    .write(flags & File::WRITE != 0)
                    .append(flags & File::APPEND != 0)
                    .open(&host_path)
                    .map_err(|_| de::Error::custom("Could not open file on this system"))?;
                Ok(File {
                    inner,
                    host_path,
                    flags,
                })
            }
        }

        const FIELDS: &[&str] = &["host_path", "flags"];
        deserializer.deserialize_struct("File", FIELDS, FileVisitor)
    }
}

impl File {
    const READ: u16 = 1;
    const WRITE: u16 = 2;
    const APPEND: u16 = 4;

    /// creates a new host file from a `std::fs::File` and a path
    pub fn new(file: fs::File, host_path: PathBuf, read: bool, write: bool, append: bool) -> Self {
        let mut _flags = 0;

        if read {
            _flags |= Self::READ;
        }

        if write {
            _flags |= Self::WRITE;
        }

        if append {
            _flags |= Self::APPEND;
        }

        Self {
            inner: file,
            host_path,
            #[cfg(feature = "enable-serde")]
            flags: _flags,
        }
    }

    pub fn metadata(&self) -> fs::Metadata {
        self.inner.metadata().unwrap()
    }
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        self.inner.read_to_end(buf)
    }

    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        self.inner.read_to_string(buf)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.inner.read_exact(buf)
    }
}

impl Seek for File {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

impl Write for File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.inner.write_all(buf)
    }

    fn write_fmt(&mut self, fmt: ::std::fmt::Arguments) -> io::Result<()> {
        self.inner.write_fmt(fmt)
    }
}

//#[cfg_attr(feature = "enable-serde", typetag::serde)]
impl VirtualFile for File {
    fn last_accessed(&self) -> u64 {
        self.metadata()
            .accessed()
            .ok()
            .and_then(|ct| ct.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|ct| ct.as_nanos() as u64)
            .unwrap_or(0)
    }

    fn last_modified(&self) -> u64 {
        self.metadata()
            .modified()
            .ok()
            .and_then(|ct| ct.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|ct| ct.as_nanos() as u64)
            .unwrap_or(0)
    }

    fn created_time(&self) -> u64 {
        self.metadata()
            .created()
            .ok()
            .and_then(|ct| ct.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|ct| ct.as_nanos() as u64)
            .unwrap_or(0)
    }

    fn size(&self) -> u64 {
        self.metadata().len()
    }

    fn set_len(&mut self, new_size: u64) -> Result<()> {
        fs::File::set_len(&self.inner, new_size).map_err(Into::into)
    }

    fn unlink(&mut self) -> Result<()> {
        fs::remove_file(&self.host_path).map_err(Into::into)
    }
    fn sync_to_disk(&self) -> Result<()> {
        self.inner.sync_all().map_err(Into::into)
    }

    fn bytes_available(&self) -> Result<usize> {
        host_file_bytes_available(self.inner.try_into_filedescriptor()?)
    }
}

#[cfg(unix)]
fn host_file_bytes_available(host_fd: FileDescriptor) -> Result<usize> {
    let mut bytes_found: libc::c_int = 0;
    let result = unsafe { libc::ioctl(host_fd.try_into()?, libc::FIONREAD, &mut bytes_found) };

    match result {
        // success
        0 => Ok(bytes_found.try_into().unwrap_or(0)),
        libc::EBADF => Err(FsError::InvalidFd),
        libc::EFAULT => Err(FsError::InvalidData),
        libc::EINVAL => Err(FsError::InvalidInput),
        _ => Err(FsError::IOError),
    }
}

#[cfg(not(unix))]
fn host_file_bytes_available(_host_fd: FileDescriptor) -> Result<usize> {
    unimplemented!("host_file_bytes_available not yet implemented for non-Unix-like targets.  This probably means the program tried to use wasi::poll_oneoff")
}

/// A wrapper type around Stdout that implements `VirtualFile` and
/// `Serialize` + `Deserialize`.
#[derive(Debug, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Stdout;

impl Read for Stdout {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stdout",
        ))
    }

    fn read_to_end(&mut self, _buf: &mut Vec<u8>) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stdout",
        ))
    }

    fn read_to_string(&mut self, _buf: &mut String) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stdout",
        ))
    }

    fn read_exact(&mut self, _buf: &mut [u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stdout",
        ))
    }
}

impl Seek for Stdout {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stdout"))
    }
}

impl Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        io::stdout().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stdout().flush()
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        io::stdout().write_all(buf)
    }

    fn write_fmt(&mut self, fmt: ::std::fmt::Arguments) -> io::Result<()> {
        io::stdout().write_fmt(fmt)
    }
}

//#[cfg_attr(feature = "enable-serde", typetag::serde)]
impl VirtualFile for Stdout {
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
        0
    }

    fn set_len(&mut self, _new_size: u64) -> Result<()> {
        debug!("Calling VirtualFile::set_len on stdout; this is probably a bug");
        Err(FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> Result<()> {
        Ok(())
    }

    fn bytes_available(&self) -> Result<usize> {
        host_file_bytes_available(io::stdout().try_into_filedescriptor()?)
    }

    fn get_fd(&self) -> Option<FileDescriptor> {
        io::stdout().try_into_filedescriptor().ok()
    }
}

/// A wrapper type around Stderr that implements `VirtualFile` and
/// `Serialize` + `Deserialize`.
#[derive(Debug, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Stderr;

impl Read for Stderr {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stderr",
        ))
    }

    fn read_to_end(&mut self, _buf: &mut Vec<u8>) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stderr",
        ))
    }

    fn read_to_string(&mut self, _buf: &mut String) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stderr",
        ))
    }

    fn read_exact(&mut self, _buf: &mut [u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stderr",
        ))
    }
}

impl Seek for Stderr {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stderr"))
    }
}

impl Write for Stderr {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        io::stderr().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        io::stderr().write_all(buf)
    }

    fn write_fmt(&mut self, fmt: ::std::fmt::Arguments) -> io::Result<()> {
        io::stderr().write_fmt(fmt)
    }
}

//#[cfg_attr(feature = "enable-serde", typetag::serde)]
impl VirtualFile for Stderr {
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
        0
    }

    fn set_len(&mut self, _new_size: u64) -> Result<()> {
        debug!("Calling VirtualFile::set_len on stderr; this is probably a bug");
        Err(FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> Result<()> {
        Ok(())
    }

    fn bytes_available(&self) -> Result<usize> {
        host_file_bytes_available(io::stderr().try_into_filedescriptor()?)
    }

    fn get_fd(&self) -> Option<FileDescriptor> {
        io::stderr().try_into_filedescriptor().ok()
    }
}

/// A wrapper type around Stdin that implements `VirtualFile` and
/// `Serialize` + `Deserialize`.
#[derive(Debug, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Stdin;
impl Read for Stdin {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        io::stdin().read(buf)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        io::stdin().read_to_end(buf)
    }

    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        io::stdin().read_to_string(buf)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        io::stdin().read_exact(buf)
    }
}

impl Seek for Stdin {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stdin"))
    }
}

impl Write for Stdin {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not write to stdin",
        ))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not write to stdin",
        ))
    }

    fn write_all(&mut self, _buf: &[u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not write to stdin",
        ))
    }

    fn write_fmt(&mut self, _fmt: ::std::fmt::Arguments) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not write to stdin",
        ))
    }
}

//#[cfg_attr(feature = "enable-serde", typetag::serde)]
impl VirtualFile for Stdin {
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
        0
    }

    fn set_len(&mut self, _new_size: u64) -> Result<()> {
        debug!("Calling VirtualFile::set_len on stdin; this is probably a bug");
        Err(FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> Result<()> {
        Ok(())
    }

    fn bytes_available(&self) -> Result<usize> {
        host_file_bytes_available(io::stdin().try_into_filedescriptor()?)
    }

    fn get_fd(&self) -> Option<FileDescriptor> {
        io::stdin().try_into_filedescriptor().ok()
    }
}
