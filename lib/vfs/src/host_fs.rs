use crate::*;
use serde::{de, Deserialize, Serialize};
use std::convert::TryInto;
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct HostFileSystem;

impl FileSystem for HostFileSystem {
    fn read_dir(&self, path: &Path) -> Result<ReadDir, FsError> {
        let read_dir = std::fs::read_dir(path)?;
        let data = read_dir
            .map(|entry| -> Result<DirEntry, _> {
                let entry = entry?;
                let metadata = entry.metadata()?;
                Ok(DirEntry {
                    path: entry.path(),
                    metadata: Ok(fs_metadata_to_metadata(metadata)?),
                })
            })
            .collect::<Result<Vec<DirEntry>, std::io::Error>>()
            .map_err::<FsError, _>(Into::into)?;
        Ok(ReadDir::new(data))
    }
    fn create_dir(&self, path: &Path) -> Result<(), FsError> {
        fs::create_dir(path).map_err(Into::into)
    }
    fn remove_dir(&self, path: &Path) -> Result<(), FsError> {
        fs::remove_dir(path).map_err(Into::into)
    }
    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        fs::rename(from, to).map_err(Into::into)
    }

    fn remove_file(&self, path: &Path) -> Result<(), FsError> {
        fs::remove_file(path).map_err(Into::into)
    }
    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(Box::new(HostFileOpener))
    }

    fn metadata(&self, path: &Path) -> Result<Metadata, FsError> {
        fs::metadata(path)
            .and_then(fs_metadata_to_metadata)
            .map_err(Into::into)
    }
}

fn fs_metadata_to_metadata(metadata: fs::Metadata) -> Result<Metadata, std::io::Error> {
    use std::time::UNIX_EPOCH;
    let filetype = metadata.file_type();
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
            (false, false, false)
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
        accessed: metadata
            .accessed()?
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64,
        created: metadata
            .created()?
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64,
        modified: metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64,
        len: metadata.len(),
    })
}

#[derive(Debug, Clone)]
pub struct HostFileOpener;

impl FileOpener for HostFileOpener {
    fn open(
        &mut self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile>, FsError> {
        // TODO: handle create implying write, etc.
        let read = conf.read();
        let write = conf.write();
        let append = conf.append();
        let mut oo = std::fs::OpenOptions::new();
        oo.read(conf.read())
            .write(conf.write())
            .create_new(conf.create_new())
            .create(conf.create())
            .append(conf.append())
            .truncate(conf.truncate())
            .open(path)
            .map_err(Into::into)
            .map(|file| {
                Box::new(HostFile::new(file, path.to_owned(), read, write, append))
                    as Box<dyn VirtualFile>
            })
    }
}

/// A thin wrapper around `std::fs::File`
#[derive(Debug, Serialize)]
pub struct HostFile {
    #[serde(skip_serializing)]
    pub inner: fs::File,
    pub host_path: PathBuf,
    flags: u16,
}

impl<'de> Deserialize<'de> for HostFile {
    fn deserialize<D>(deserializer: D) -> Result<HostFile, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            HostPath,
            Flags,
        }

        struct HostFileVisitor;

        impl<'de> de::Visitor<'de> for HostFileVisitor {
            type Value = HostFile;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct HostFile")
            }

            fn visit_seq<V>(self, mut seq: V) -> Result<Self::Value, V::Error>
            where
                V: de::SeqAccess<'de>,
            {
                let host_path = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let flags = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let inner = std::fs::OpenOptions::new()
                    .read(flags & HostFile::READ != 0)
                    .write(flags & HostFile::WRITE != 0)
                    .append(flags & HostFile::APPEND != 0)
                    .open(&host_path)
                    .map_err(|_| de::Error::custom("Could not open file on this system"))?;
                Ok(HostFile {
                    inner,
                    host_path,
                    flags,
                })
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
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
                let inner = std::fs::OpenOptions::new()
                    .read(flags & HostFile::READ != 0)
                    .write(flags & HostFile::WRITE != 0)
                    .append(flags & HostFile::APPEND != 0)
                    .open(&host_path)
                    .map_err(|_| de::Error::custom("Could not open file on this system"))?;
                Ok(HostFile {
                    inner,
                    host_path,
                    flags,
                })
            }
        }

        const FIELDS: &[&str] = &["host_path", "flags"];
        deserializer.deserialize_struct("HostFile", FIELDS, HostFileVisitor)
    }
}

impl HostFile {
    const READ: u16 = 1;
    const WRITE: u16 = 2;
    const APPEND: u16 = 4;

    /// creates a new host file from a `std::fs::File` and a path
    pub fn new(file: fs::File, host_path: PathBuf, read: bool, write: bool, append: bool) -> Self {
        let mut flags = 0;
        if read {
            flags |= Self::READ;
        }
        if write {
            flags |= Self::WRITE;
        }
        if append {
            flags |= Self::APPEND;
        }
        Self {
            inner: file,
            host_path,
            flags,
        }
    }

    pub fn metadata(&self) -> fs::Metadata {
        self.inner.metadata().unwrap()
    }
}

impl Read for HostFile {
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
impl Seek for HostFile {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}
impl Write for HostFile {
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

impl VirtualFile for HostFile {
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

    fn set_len(&mut self, new_size: u64) -> Result<(), FsError> {
        fs::File::set_len(&self.inner, new_size).map_err(Into::into)
    }

    fn unlink(&mut self) -> Result<(), FsError> {
        std::fs::remove_file(&self.host_path).map_err(Into::into)
    }
    fn sync_to_disk(&self) -> Result<(), FsError> {
        self.inner.sync_all().map_err(Into::into)
    }

    fn rename_file(&self, new_name: &std::path::Path) -> Result<(), FsError> {
        std::fs::rename(&self.host_path, new_name).map_err(Into::into)
    }

    fn bytes_available(&self) -> Result<usize, FsError> {
        // unwrap is safe because of get_raw_fd implementation
        let host_fd = self.get_raw_fd().unwrap();

        host_file_bytes_available(host_fd)
    }

    #[cfg(unix)]
    fn get_raw_fd(&self) -> Option<i32> {
        use std::os::unix::io::AsRawFd;
        Some(self.inner.as_raw_fd())
    }
    #[cfg(not(unix))]
    fn get_raw_fd(&self) -> Option<i32> {
        unimplemented!(
            "HostFile::get_raw_fd in VirtualFile is not implemented for non-Unix-like targets yet"
        );
    }
}

#[cfg(unix)]
fn host_file_bytes_available(host_fd: i32) -> Result<usize, FsError> {
    let mut bytes_found = 0 as libc::c_int;
    let result = unsafe { libc::ioctl(host_fd, libc::FIONREAD, &mut bytes_found) };

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
fn host_file_bytes_available(_raw_fd: i32) -> Result<usize, FsError> {
    unimplemented!("host_file_bytes_available not yet implemented for non-Unix-like targets.  This probably means the program tried to use wasi::poll_oneoff")
}

/// A wrapper type around Stdout that implements `VirtualFile` and
/// `Serialize` + `Deserialize`.
#[derive(Debug, Serialize, Deserialize, Default)]
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
    fn set_len(&mut self, _new_size: u64) -> Result<(), FsError> {
        debug!("Calling VirtualFile::set_len on stdout; this is probably a bug");
        Err(FsError::PermissionDenied)
    }
    fn unlink(&mut self) -> Result<(), FsError> {
        Ok(())
    }

    fn bytes_available(&self) -> Result<usize, FsError> {
        // unwrap is safe because of get_raw_fd implementation
        let host_fd = self.get_raw_fd().unwrap();

        host_file_bytes_available(host_fd)
    }

    #[cfg(unix)]
    fn get_raw_fd(&self) -> Option<i32> {
        use std::os::unix::io::AsRawFd;
        Some(io::stdout().as_raw_fd())
    }

    #[cfg(not(unix))]
    fn get_raw_fd(&self) -> Option<i32> {
        unimplemented!(
            "Stdout::get_raw_fd in VirtualFile is not implemented for non-Unix-like targets yet"
        );
    }
}

/// A wrapper type around Stderr that implements `VirtualFile` and
/// `Serialize` + `Deserialize`.
#[derive(Debug, Serialize, Deserialize, Default)]
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
    fn set_len(&mut self, _new_size: u64) -> Result<(), FsError> {
        debug!("Calling VirtualFile::set_len on stderr; this is probably a bug");
        Err(FsError::PermissionDenied)
    }
    fn unlink(&mut self) -> Result<(), FsError> {
        Ok(())
    }

    fn bytes_available(&self) -> Result<usize, FsError> {
        // unwrap is safe because of get_raw_fd implementation
        let host_fd = self.get_raw_fd().unwrap();

        host_file_bytes_available(host_fd)
    }

    #[cfg(unix)]
    fn get_raw_fd(&self) -> Option<i32> {
        use std::os::unix::io::AsRawFd;
        Some(io::stderr().as_raw_fd())
    }

    #[cfg(not(unix))]
    fn get_raw_fd(&self) -> Option<i32> {
        unimplemented!(
            "Stderr::get_raw_fd in VirtualFile is not implemented for non-Unix-like targets yet"
        );
    }
}

/// A wrapper type around Stdin that implements `VirtualFile` and
/// `Serialize` + `Deserialize`.
#[derive(Debug, Serialize, Deserialize, Default)]
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
    fn set_len(&mut self, _new_size: u64) -> Result<(), FsError> {
        debug!("Calling VirtualFile::set_len on stdin; this is probably a bug");
        Err(FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> Result<(), FsError> {
        Ok(())
    }

    fn bytes_available(&self) -> Result<usize, FsError> {
        // unwrap is safe because of get_raw_fd implementation
        let host_fd = self.get_raw_fd().unwrap();

        host_file_bytes_available(host_fd)
    }

    #[cfg(unix)]
    fn get_raw_fd(&self) -> Option<i32> {
        use std::os::unix::io::AsRawFd;
        Some(io::stdin().as_raw_fd())
    }

    #[cfg(not(unix))]
    fn get_raw_fd(&self) -> Option<i32> {
        unimplemented!(
            "Stdin::get_raw_fd in VirtualFile is not implemented for non-Unix-like targets yet"
        );
    }
}
