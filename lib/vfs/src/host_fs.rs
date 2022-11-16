use crate::{
    DirEntry, FileDescriptor, FileType, FsError, Metadata, OpenOptions, OpenOptionsConfig, ReadDir,
    Result, VirtualFile,
};
#[cfg(feature = "enable-serde")]
use serde::{de, Deserialize, Serialize};
use std::cmp::Ordering;
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

#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct FileSystem;

impl FileSystem {
    pub fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        if !path.exists() {
            return Err(FsError::InvalidInput);
        }
        fs::canonicalize(path).map_err(Into::into)
    }
}

impl crate::FileSystem for FileSystem {
    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        let read_dir = fs::read_dir(path)?;
        let mut data = read_dir
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
        data.sort_by(|a, b| match (a.metadata.as_ref(), b.metadata.as_ref()) {
            (Ok(a), Ok(b)) => a.modified.cmp(&b.modified),
            _ => Ordering::Equal,
        });
        Ok(ReadDir::new(data))
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        if path.parent().is_none() {
            return Err(FsError::BaseNotDirectory);
        }
        fs::create_dir(path).map_err(Into::into)
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        if path.parent().is_none() {
            return Err(FsError::BaseNotDirectory);
        }
        // https://github.com/rust-lang/rust/issues/86442
        // DirectoryNotEmpty is not implemented consistently
        if path.is_dir() && self.read_dir(path).map(|s| !s.is_empty()).unwrap_or(false) {
            return Err(FsError::DirectoryNotEmpty);
        }
        fs::remove_dir(path).map_err(Into::into)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        use filetime::{set_file_mtime, FileTime};
        if from.parent().is_none() {
            return Err(FsError::BaseNotDirectory);
        }
        if to.parent().is_none() {
            return Err(FsError::BaseNotDirectory);
        }
        if !from.exists() {
            return Err(FsError::EntryNotFound);
        }
        let from_parent = from.parent().unwrap();
        let to_parent = to.parent().unwrap();
        if !from_parent.exists() {
            return Err(FsError::EntryNotFound);
        }
        if !to_parent.exists() {
            return Err(FsError::EntryNotFound);
        }
        let result = if from_parent != to_parent {
            let _ = std::fs::create_dir_all(to_parent);
            if from.is_dir() {
                fs_extra::move_items(
                    &[from],
                    to,
                    &fs_extra::dir::CopyOptions {
                        copy_inside: true,
                        ..Default::default()
                    },
                )
                .map(|_| ())
                .map_err(|_| FsError::UnknownError)?;
                let _ = fs_extra::remove_items(&[from]);
                Ok(())
            } else {
                let e: Result<()> = fs::copy(from, to).map(|_| ()).map_err(Into::into);
                let _ = e?;
                fs::remove_file(from).map(|_| ()).map_err(Into::into)
            }
        } else {
            fs::rename(from, to).map_err(Into::into)
        };
        let _ = set_file_mtime(to, FileTime::now()).map(|_| ());
        result
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        if path.parent().is_none() {
            return Err(FsError::BaseNotDirectory);
        }
        fs::remove_file(path).map_err(Into::into)
    }

    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(Box::new(FileOpener))
    }

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        fs::metadata(path)
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
pub struct FileOpener;

impl crate::FileOpener for FileOpener {
    fn open(
        &mut self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>> {
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
            .open(path)
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

    #[cfg(feature = "sys")]
    fn bytes_available(&self) -> Result<usize> {
        host_file_bytes_available(self.inner.try_into_filedescriptor()?)
    }

    #[cfg(not(feature = "sys"))]
    fn bytes_available(&self) -> Result<usize> {
        Err(FsError::InvalidFd)
    }

    fn get_special_fd(&self) -> Option<u32> {
        None
    }
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

    #[cfg(feature = "sys")]
    fn bytes_available(&self) -> Result<usize> {
        host_file_bytes_available(io::stdout().try_into_filedescriptor()?)
    }

    #[cfg(feature = "sys")]
    fn get_fd(&self) -> Option<FileDescriptor> {
        io::stdout().try_into_filedescriptor().ok()
    }

    #[cfg(feature = "js")]
    fn bytes_available(&self) -> Result<usize> {
        Err(FsError::InvalidFd);
    }

    #[cfg(feature = "js")]
    fn get_fd(&self) -> Option<FileDescriptor> {
        None
    }

    fn get_special_fd(&self) -> Option<u32> {
        Some(1)
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

    #[cfg(feature = "sys")]
    fn bytes_available(&self) -> Result<usize> {
        host_file_bytes_available(io::stderr().try_into_filedescriptor()?)
    }

    #[cfg(feature = "sys")]
    fn get_fd(&self) -> Option<FileDescriptor> {
        io::stderr().try_into_filedescriptor().ok()
    }

    #[cfg(feature = "js")]
    fn bytes_available(&self) -> Result<usize> {
        Err(FsError::InvalidFd);
    }

    #[cfg(feature = "js")]
    fn get_fd(&self) -> Option<FileDescriptor> {
        None
    }

    fn get_special_fd(&self) -> Option<u32> {
        Some(2)
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

    #[cfg(feature = "sys")]
    fn bytes_available(&self) -> Result<usize> {
        host_file_bytes_available(io::stdin().try_into_filedescriptor()?)
    }

    #[cfg(feature = "sys")]
    fn get_fd(&self) -> Option<FileDescriptor> {
        io::stdin().try_into_filedescriptor().ok()
    }

    #[cfg(feature = "js")]
    fn bytes_available(&self) -> Result<usize> {
        Err(FsError::InvalidFd);
    }

    #[cfg(feature = "js")]
    fn get_fd(&self) -> Option<FileDescriptor> {
        None
    }

    fn get_special_fd(&self) -> Option<u32> {
        Some(0)
    }
}

#[cfg(test)]
mod tests {
    use crate::host_fs::FileSystem;
    use crate::FileSystem as FileSystemTrait;
    use crate::FsError;
    use std::path::Path;

    #[test]
    fn test_new_filesystem() {
        let fs = FileSystem::default();
        assert!(fs.read_dir(Path::new("/")).is_ok(), "hostfs can read root");
        std::fs::write("./foo2.txt", b"").unwrap();
        assert!(
            fs.new_open_options()
                .read(true)
                .open(Path::new("./foo2.txt"))
                .is_ok(),
            "created foo2.txt"
        );
        std::fs::remove_file("./foo2.txt").unwrap();
    }

    #[test]
    fn test_create_dir() {
        let fs = FileSystem::default();

        assert_eq!(
            fs.create_dir(Path::new("/")),
            Err(FsError::BaseNotDirectory),
            "creating a directory that has no parent",
        );

        let _ = fs_extra::remove_items(&["./test_create_dir"]);

        assert_eq!(
            fs.create_dir(Path::new("./test_create_dir")),
            Ok(()),
            "creating a directory",
        );

        assert_eq!(
            fs.create_dir(Path::new("./test_create_dir/foo")),
            Ok(()),
            "creating a directory",
        );

        assert!(
            Path::new("./test_create_dir/foo").exists(),
            "foo dir exists in host_fs"
        );

        let cur_dir = read_dir_names(&fs, "./test_create_dir");

        if !cur_dir.contains(&"foo".to_string()) {
            panic!("cur_dir does not contain foo: {cur_dir:#?}");
        }

        assert!(
            cur_dir.contains(&"foo".to_string()),
            "the root is updated and well-defined"
        );

        assert_eq!(
            fs.create_dir(Path::new("./test_create_dir/foo/bar")),
            Ok(()),
            "creating a sub-directory",
        );

        assert!(
            Path::new("./test_create_dir/foo/bar").exists(),
            "foo dir exists in host_fs"
        );

        let foo_dir = read_dir_names(&fs, "./test_create_dir/foo");

        assert!(
            foo_dir.contains(&"bar".to_string()),
            "the foo directory is updated and well-defined"
        );

        let bar_dir = read_dir_names(&fs, "./test_create_dir/foo/bar");

        assert!(
            bar_dir.is_empty(),
            "the foo directory is updated and well-defined"
        );
        let _ = fs_extra::remove_items(&["./test_create_dir"]);
    }

    #[test]
    fn test_remove_dir() {
        let fs = FileSystem::default();

        let _ = fs_extra::remove_items(&["./test_remove_dir"]);

        assert_eq!(
            fs.remove_dir(Path::new("/")),
            Err(FsError::BaseNotDirectory),
            "removing a directory that has no parent",
        );

        assert_eq!(
            fs.remove_dir(Path::new("/foo")),
            Err(FsError::EntryNotFound),
            "cannot remove a directory that doesn't exist",
        );

        assert_eq!(
            fs.create_dir(Path::new("./test_remove_dir")),
            Ok(()),
            "creating a directory",
        );

        assert_eq!(
            fs.create_dir(Path::new("./test_remove_dir/foo")),
            Ok(()),
            "creating a directory",
        );

        assert_eq!(
            fs.create_dir(Path::new("./test_remove_dir/foo/bar")),
            Ok(()),
            "creating a sub-directory",
        );

        assert!(
            Path::new("./test_remove_dir/foo/bar").exists(),
            "./foo/bar exists"
        );

        assert_eq!(
            fs.remove_dir(Path::new("./test_remove_dir/foo")),
            Err(FsError::DirectoryNotEmpty),
            "removing a directory that has children",
        );

        assert_eq!(
            fs.remove_dir(Path::new("./test_remove_dir/foo/bar")),
            Ok(()),
            "removing a sub-directory",
        );

        assert_eq!(
            fs.remove_dir(Path::new("./test_remove_dir/foo")),
            Ok(()),
            "removing a directory",
        );

        let cur_dir = read_dir_names(&fs, "./test_remove_dir");

        assert!(
            !cur_dir.contains(&"foo".to_string()),
            "the foo directory still exists"
        );

        let _ = fs_extra::remove_items(&["./test_remove_dir"]);
    }

    fn read_dir_names(fs: &dyn crate::FileSystem, path: &str) -> Vec<String> {
        fs.read_dir(Path::new(path))
            .unwrap()
            .filter_map(|entry| Some(entry.ok()?.file_name().to_str()?.to_string()))
            .collect::<Vec<_>>()
    }

    #[test]
    fn test_rename() {
        let fs = FileSystem::default();

        let _ = fs_extra::remove_items(&["./test_rename"]);

        assert_eq!(
            fs.rename(Path::new("/"), Path::new("/bar")),
            Err(FsError::BaseNotDirectory),
            "renaming a directory that has no parent",
        );
        assert_eq!(
            fs.rename(Path::new("/foo"), Path::new("/")),
            Err(FsError::BaseNotDirectory),
            "renaming to a directory that has no parent",
        );

        assert_eq!(fs.create_dir(Path::new("./test_rename")), Ok(()));
        assert_eq!(fs.create_dir(Path::new("./test_rename/foo")), Ok(()));
        assert_eq!(fs.create_dir(Path::new("./test_rename/foo/qux")), Ok(()));

        assert_eq!(
            fs.rename(
                Path::new("./test_rename/foo"),
                Path::new("./test_rename/bar/baz")
            ),
            Err(FsError::EntryNotFound),
            "renaming to a directory that has parent that doesn't exist",
        );

        assert_eq!(fs.create_dir(Path::new("./test_rename/bar")), Ok(()));

        assert_eq!(
            fs.rename(
                Path::new("./test_rename/foo"),
                Path::new("./test_rename/bar")
            ),
            Ok(()),
            "renaming to a directory that has parent that exists",
        );

        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(Path::new("./test_rename/bar/hello1.txt")),
                Ok(_),
            ),
            "creating a new file (`hello1.txt`)",
        );
        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(Path::new("./test_rename/bar/hello2.txt")),
                Ok(_),
            ),
            "creating a new file (`hello2.txt`)",
        );

        let cur_dir = read_dir_names(&fs, "./test_rename");

        assert!(
            !cur_dir.contains(&"foo".to_string()),
            "the foo directory still exists"
        );

        assert!(
            cur_dir.contains(&"bar".to_string()),
            "the bar directory still exists"
        );

        let bar_dir = read_dir_names(&fs, "./test_rename/bar");

        if !bar_dir.contains(&"qux".to_string()) {
            println!("qux does not exist: {:?}", bar_dir)
        }

        let qux_dir = read_dir_names(&fs, "./test_rename/bar/qux");

        assert!(qux_dir.is_empty(), "the qux directory is empty");

        assert!(
            Path::new("./test_rename/bar/hello1.txt").exists(),
            "the /bar/hello1.txt file exists"
        );

        assert!(
            Path::new("./test_rename/bar/hello2.txt").exists(),
            "the /bar/hello2.txt file exists"
        );

        assert_eq!(
            fs.create_dir(Path::new("./test_rename/foo")),
            Ok(()),
            "create ./foo again",
        );

        assert_eq!(
            fs.rename(
                Path::new("./test_rename/bar/hello2.txt"),
                Path::new("./test_rename/foo/world2.txt")
            ),
            Ok(()),
            "renaming (and moving) a file",
        );

        assert_eq!(
            fs.rename(
                Path::new("./test_rename/foo"),
                Path::new("./test_rename/bar/baz")
            ),
            Ok(()),
            "renaming a directory",
        );

        assert_eq!(
            fs.rename(
                Path::new("./test_rename/bar/hello1.txt"),
                Path::new("./test_rename/bar/world1.txt")
            ),
            Ok(()),
            "renaming a file (in the same directory)",
        );

        assert!(Path::new("./test_rename/bar").exists(), "./bar exists");
        assert!(
            Path::new("./test_rename/bar/baz").exists(),
            "./bar/baz exists"
        );
        assert!(
            !Path::new("./test_rename/foo").exists(),
            "foo does not exist anymore"
        );
        assert!(
            Path::new("./test_rename/bar/baz/world2.txt").exists(),
            "/bar/baz/world2.txt exists"
        );
        assert!(
            Path::new("./test_rename/bar/world1.txt").exists(),
            "/bar/world1.txt (ex hello1.txt) exists"
        );
        assert!(
            !Path::new("./test_rename/bar/hello1.txt").exists(),
            "hello1.txt was moved"
        );
        assert!(
            !Path::new("./test_rename/bar/hello2.txt").exists(),
            "hello2.txt was moved"
        );
        assert!(
            Path::new("./test_rename/bar/baz/world2.txt").exists(),
            "world2.txt was moved to the correct place"
        );

        let _ = fs_extra::remove_items(&["./test_rename"]);
    }

    #[test]
    fn test_metadata() {
        use std::thread::sleep;
        use std::time::Duration;

        let root_dir = env!("CARGO_MANIFEST_DIR");
        let _ = std::env::set_current_dir(root_dir);

        let fs = FileSystem::default();

        let _ = fs_extra::remove_items(&["./test_metadata"]);

        assert_eq!(fs.create_dir(Path::new("./test_metadata")), Ok(()));

        let root_metadata = fs.metadata(Path::new("./test_metadata")).unwrap();

        assert!(root_metadata.ft.dir);
        assert!(root_metadata.accessed == root_metadata.created);
        assert!(root_metadata.modified == root_metadata.created);
        assert!(root_metadata.modified > 0);

        assert_eq!(fs.create_dir(Path::new("./test_metadata/foo")), Ok(()));

        let foo_metadata = fs.metadata(Path::new("./test_metadata/foo"));
        assert!(foo_metadata.is_ok());
        let foo_metadata = foo_metadata.unwrap();

        assert!(foo_metadata.ft.dir);
        assert!(foo_metadata.accessed == foo_metadata.created);
        assert!(foo_metadata.modified == foo_metadata.created);
        assert!(foo_metadata.modified > 0);

        sleep(Duration::from_secs(3));

        assert_eq!(
            fs.rename(
                Path::new("./test_metadata/foo"),
                Path::new("./test_metadata/bar")
            ),
            Ok(())
        );

        let bar_metadata = fs.metadata(Path::new("./test_metadata/bar")).unwrap();
        assert!(bar_metadata.ft.dir);
        assert!(bar_metadata.accessed == foo_metadata.accessed);
        assert!(bar_metadata.created == foo_metadata.created);
        assert!(bar_metadata.modified > foo_metadata.modified);

        let root_metadata = fs.metadata(Path::new("./test_metadata/bar")).unwrap();
        assert!(
            root_metadata.modified > foo_metadata.modified,
            "the parent modified time was updated"
        );

        let _ = fs_extra::remove_items(&["./test_metadata"]);
    }

    #[test]
    fn test_remove_file() {
        let fs = FileSystem::default();

        let _ = fs_extra::remove_items(&["./test_remove_file"]);

        assert!(fs.create_dir(Path::new("./test_remove_file")).is_ok());

        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(Path::new("./test_remove_file/foo.txt")),
                Ok(_)
            ),
            "creating a new file",
        );

        assert!(read_dir_names(&fs, "./test_remove_file").contains(&"foo.txt".to_string()));

        assert!(Path::new("./test_remove_file/foo.txt").is_file());

        assert_eq!(
            fs.remove_file(Path::new("./test_remove_file/foo.txt")),
            Ok(()),
            "removing a file that exists",
        );

        assert!(!Path::new("./test_remove_file/foo.txt").exists());

        assert_eq!(
            fs.remove_file(Path::new("./test_remove_file/foo.txt")),
            Err(FsError::EntryNotFound),
            "removing a file that doesn't exists",
        );

        let _ = fs_extra::remove_items(&["./test_remove_file"]);
    }

    #[test]
    fn test_readdir() {
        let fs = FileSystem::default();

        let _ = fs_extra::remove_items(&["./test_readdir"]);

        assert_eq!(
            fs.create_dir(Path::new("./test_readdir/")),
            Ok(()),
            "creating `test_readdir`"
        );

        assert_eq!(
            fs.create_dir(Path::new("./test_readdir/foo")),
            Ok(()),
            "creating `foo`"
        );
        assert_eq!(
            fs.create_dir(Path::new("./test_readdir/foo/sub")),
            Ok(()),
            "creating `sub`"
        );
        assert_eq!(
            fs.create_dir(Path::new("./test_readdir/bar")),
            Ok(()),
            "creating `bar`"
        );
        assert_eq!(
            fs.create_dir(Path::new("./test_readdir/baz")),
            Ok(()),
            "creating `bar`"
        );
        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(Path::new("./test_readdir/a.txt")),
                Ok(_)
            ),
            "creating `a.txt`",
        );
        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(Path::new("./test_readdir/b.txt")),
                Ok(_)
            ),
            "creating `b.txt`",
        );

        let readdir = fs.read_dir(Path::new("./test_readdir"));

        assert!(readdir.is_ok(), "reading the directory `./test_readdir/`");

        let mut readdir = readdir.unwrap();

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("foo"), "checking entry #1");
        assert!(next.path.is_dir(), "checking entry #1");

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("bar"), "checking entry #2");
        assert!(next.path.is_dir(), "checking entry #2");

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("baz"), "checking entry #3");
        assert!(next.path.is_dir(), "checking entry #3");

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("a.txt"), "checking entry #2");
        assert!(next.path.is_file(), "checking entry #4");

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("b.txt"), "checking entry #2");
        assert!(next.path.is_file(), "checking entry #5");

        if let Some(s) = readdir.next() {
            panic!("next: {s:?}");
        }

        let _ = fs_extra::remove_items(&["./test_readdir"]);
    }

    #[test]
    fn test_canonicalize() {
        let fs = FileSystem::default();

        let root_dir = env!("CARGO_MANIFEST_DIR");

        let _ = fs_extra::remove_items(&["./test_canonicalize"]);

        assert_eq!(
            fs.create_dir(Path::new("./test_canonicalize")),
            Ok(()),
            "creating `test_canonicalize`"
        );

        assert_eq!(
            fs.create_dir(Path::new("./test_canonicalize/foo")),
            Ok(()),
            "creating `foo`"
        );
        assert_eq!(
            fs.create_dir(Path::new("./test_canonicalize/foo/bar")),
            Ok(()),
            "creating `bar`"
        );
        assert_eq!(
            fs.create_dir(Path::new("./test_canonicalize/foo/bar/baz")),
            Ok(()),
            "creating `baz`",
        );
        assert_eq!(
            fs.create_dir(Path::new("./test_canonicalize/foo/bar/baz/qux")),
            Ok(()),
            "creating `qux`",
        );
        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(Path::new("./test_canonicalize/foo/bar/baz/qux/hello.txt")),
                Ok(_)
            ),
            "creating `hello.txt`",
        );

        assert_eq!(
            fs.canonicalize(Path::new("./test_canonicalize")),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize")).to_path_buf()),
            "canonicalizing `/`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("foo")),
            Err(FsError::InvalidInput),
            "canonicalizing `foo`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("./test_canonicalize/././././foo/")),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize/foo")).to_path_buf()),
            "canonicalizing `/././././foo/`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("./test_canonicalize/foo/bar//")),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize/foo/bar")).to_path_buf()),
            "canonicalizing `/foo/bar//`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("./test_canonicalize/foo/bar/../bar")),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize/foo/bar")).to_path_buf()),
            "canonicalizing `/foo/bar/../bar`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("./test_canonicalize/foo/bar/../..")),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize")).to_path_buf()),
            "canonicalizing `/foo/bar/../..`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("/foo/bar/../../..")),
            Err(FsError::InvalidInput),
            "canonicalizing `/foo/bar/../../..`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("C:/foo/")),
            Err(FsError::InvalidInput),
            "canonicalizing `C:/foo/`",
        );
        assert_eq!(
            fs.canonicalize(Path::new(
                "./test_canonicalize/foo/./../foo/bar/../../foo/bar/./baz/./../baz/qux/../../baz/./qux/hello.txt"
            )),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize/foo/bar/baz/qux/hello.txt")).to_path_buf()),
            "canonicalizing a crazily stupid path name",
        );

        let _ = fs_extra::remove_items(&["./test_canonicalize"]);
    }
}
