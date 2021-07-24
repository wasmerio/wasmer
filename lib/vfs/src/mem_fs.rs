use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
enum MemKind {
    File {
        name: String,
        inode: u64,
    },
    Directory {
        name: String,
        contents: HashMap<String, MemKind>,
    },
}

impl Default for MemKind {
    fn default() -> Self {
        MemKind::Directory {
            name: "/".to_string(),
            contents: Default::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MemFileSystem {
    inner: Arc<Mutex<MemFileSystemInner>>,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct MemFileSystemInner {
    // done for recursion purposes
    fs: MemKind,
    inodes: HashMap<u64, Box<dyn VirtualFile>>,
    next_inode: u64,
}

impl MemFileSystemInner {
    fn get_memkind_at(&self, path: &Path) -> Option<&MemKind> {
        let mut components = path.components();
        if path.is_absolute() {
            components.next()?;
        }

        let mut memkind: &MemKind = &self.fs;

        for component in components {
            match memkind {
                MemKind::Directory { contents, .. } => {
                    memkind = contents.get(component.as_os_str().to_str().unwrap())?;
                }
                _ => return None,
            }
        }
        Some(memkind)
    }
    fn get_memkind_at_mut(&mut self, path: &Path) -> Option<&mut MemKind> {
        let mut components = path.components();
        if path.is_absolute() {
            components.next()?;
        }

        let mut memkind: &mut MemKind = &mut self.fs;

        for component in components {
            match memkind {
                MemKind::Directory { contents, .. } => {
                    memkind = contents.get_mut(component.as_os_str().to_str().unwrap())?;
                }
                _ => return None,
            }
        }
        Some(memkind)
    }
}

impl FileSystem for MemFileSystem {
    fn read_dir(&self, _path: &Path) -> Result<ReadDir, FsError> {
        todo!()
    }
    fn create_dir(&self, path: &Path) -> Result<(), FsError> {
        // TODO: handle errors
        let parent = path.parent().unwrap();
        let file = path.file_name().unwrap();
        let mut inner = self.inner.lock().unwrap();
        let memkind = inner.get_memkind_at_mut(parent).unwrap();
        match memkind {
            MemKind::Directory { contents, .. } => {
                let name = file.to_str().unwrap().to_string();
                if contents.contains_key(&name) {
                    // TODO: handle error
                    panic!("file exists at given path");
                }
                let mk = MemKind::Directory {
                    name: name.clone(),
                    contents: Default::default(),
                };
                contents.insert(name.clone(), mk);
            }
            _ => panic!("found file, expected directory"),
        }
        Ok(())
    }
    fn remove_dir(&self, path: &Path) -> Result<(), FsError> {
        let parent = path.parent().unwrap();
        let file = path.file_name().unwrap();
        let mut inner = self.inner.lock().unwrap();
        let memkind = inner.get_memkind_at_mut(parent).unwrap();
        match memkind {
            MemKind::Directory { contents, .. } => {
                let name = file.to_str().unwrap().to_string();
                match contents.get(&name).unwrap() {
                    MemKind::Directory { contents, .. } => {
                        if !contents.is_empty() {
                            // TODO: handle error
                            panic!("Can't delete directory, directory is not empty");
                        }
                    }
                    _ => panic!("expected directory, found file"),
                }
                contents.remove(&name);
            }
            _ => panic!("found file, expected directory"),
        }
        Ok(())
    }
    fn rename(&self, _from: &Path, _to: &Path) -> Result<(), FsError> {
        todo!("rename")
        //fs::rename(from, to).map_err(Into::into)
    }

    fn remove_file(&self, path: &Path) -> Result<(), FsError> {
        let parent = path.parent().unwrap();
        let file = path.file_name().unwrap();
        let mut inner = self.inner.lock().unwrap();
        let memkind = inner.get_memkind_at_mut(parent).unwrap();
        let inode: u64 = match memkind {
            MemKind::Directory { contents, .. } => {
                let name = file.to_str().unwrap().to_string();
                let inode: u64 = match contents.get(&name).unwrap() {
                    MemKind::File { inode, .. } => *inode,
                    _ => panic!("expected file, found directory"),
                };
                contents.remove(&name);
                inode
            }
            _ => panic!("found file, expected directory"),
        };
        inner.inodes.remove(&inode);
        Ok(())
    }
    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(Box::new(MemFileOpener(self.clone())))
    }

    fn metadata(&self, _path: &Path) -> Result<Metadata, FsError> {
        // let inner = self.inner.lock().unwrap();
        // let memkind = inner.get_memkind_at(path)
        Ok(Metadata {
            ft: FileType {
                dir: false,
                file: true,
                symlink: false,
                char_device: false,
                block_device: false,
                socket: false,
                fifo: false,
            },
            accessed: 0 as u64,
            created: 0 as u64,
            modified: 0 as u64,
            len: 0,
        })
    }
}

#[derive(Clone)]
pub struct MemFileOpener(MemFileSystem);

impl FileOpener for MemFileOpener {
    fn open(
        &mut self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile>, FsError> {
        // TODO: handle create implying write, etc.
        let read = conf.read();
        let write = conf.write();
        let append = conf.append();
        let virtual_file =
            Box::new(MemFile::new(vec![], read, write, append)) as Box<dyn VirtualFile>;
        let mut inner = self.0.inner.lock().unwrap();
        let inode = inner.next_inode;

        let parent_path = path.parent().unwrap();
        let file_name = path.file_name().unwrap().to_str().unwrap().to_string();
        // TODO: replace with an actual missing directory error
        let parent_memkind = inner
            .get_memkind_at_mut(parent_path)
            .ok_or(FsError::IOError)?;
        match parent_memkind {
            MemKind::Directory { contents, .. } => {
                if contents.contains_key(&file_name) {
                    return Err(FsError::AlreadyExists);
                }
                contents.insert(
                    file_name.clone(),
                    MemKind::File {
                        name: file_name,
                        inode,
                    },
                );
            }
            _ => {
                // expected directory
                // TODO: return a more proper error here
                return Err(FsError::IOError);
            }
        }

        inner.next_inode += 1;
        inner.inodes.insert(inode, virtual_file);

        Ok(Box::new(MemFileHandle {
            fs: self.0.clone(),
            inode,
        }) as Box<dyn VirtualFile>)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MemFile {
    buffer: Vec<u8>,
    cursor: usize,
    flags: u16,
    last_accessed: u64,
    last_modified: u64,
    created_time: u64,
}

impl MemFile {
    const READ: u16 = 1;
    const WRITE: u16 = 2;
    const APPEND: u16 = 4;

    /// creates a new host file from a `std::fs::File` and a path
    pub fn new(buffer: Vec<u8>, read: bool, write: bool, append: bool) -> Self {
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
            buffer,
            cursor: 0,
            flags,
            last_accessed: 0,
            last_modified: 0,
            created_time: 0,
        }
    }
}

impl Read for MemFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let upper_limit = std::cmp::min(self.buffer.len() - self.cursor, buf.len());
        for i in 0..upper_limit {
            buf[i] = self.buffer[self.cursor + i];
        }
        self.cursor += upper_limit;
        Ok(upper_limit)
    }
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let data_to_copy = self.buffer.len() - self.cursor;
        buf.reserve(data_to_copy);
        for i in self.cursor..self.buffer.len() {
            buf.push(self.buffer[i]);
        }
        Ok(data_to_copy)
    }
    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        // TODO: error handling
        let s = std::str::from_utf8(&self.buffer[self.cursor..]).unwrap();
        buf.push_str(s);
        let amount_read = self.buffer.len() - self.cursor;
        self.cursor = self.buffer.len();
        Ok(amount_read)
    }
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        if buf.len() < (self.buffer.len() - self.cursor) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Not enough bytes available",
            ));
        }
        for i in 0..buf.len() {
            buf[i] = self.buffer[self.cursor + i];
        }
        self.cursor += buf.len();
        Ok(())
    }
}
impl Seek for MemFile {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        match pos {
            io::SeekFrom::Start(s) => self.cursor = s as usize,
            // TODO: handle underflow / overflow properly
            io::SeekFrom::End(s) => self.cursor = (self.buffer.len() as i64 + s) as usize,
            io::SeekFrom::Current(s) => self.cursor = (self.cursor as i64 + s) as usize,
        }
        Ok(self.cursor as u64)
    }
}
impl Write for MemFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.buffer.flush()
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.buffer.write_all(buf)
    }
    fn write_fmt(&mut self, fmt: ::std::fmt::Arguments) -> io::Result<()> {
        self.buffer.write_fmt(fmt)
    }
}

#[typetag::serde]
impl VirtualFile for MemFile {
    fn last_accessed(&self) -> u64 {
        self.last_accessed
    }

    fn last_modified(&self) -> u64 {
        self.last_modified
    }

    fn created_time(&self) -> u64 {
        self.created_time
    }

    fn size(&self) -> u64 {
        self.buffer.len() as u64
    }

    fn set_len(&mut self, new_size: u64) -> Result<(), FsError> {
        self.buffer.resize(new_size as usize, 0);
        Ok(())
    }

    fn unlink(&mut self) -> Result<(), FsError> {
        self.buffer.clear();
        self.cursor = 0;
        Ok(())
    }
    fn sync_to_disk(&self) -> Result<(), FsError> {
        Ok(())
    }

    fn rename_file(&self, _new_name: &std::path::Path) -> Result<(), FsError> {
        Ok(())
    }

    fn bytes_available(&self) -> Result<usize, FsError> {
        Ok(self.buffer.len() - self.cursor)
    }

    fn get_raw_fd(&self) -> Option<i32> {
        None
    }
}

#[derive(Serialize, Deserialize)]
pub struct MemFileHandle {
    // hack, just skip it
    // #[serde(skip)]
    fs: MemFileSystem,
    inode: u64,
}

impl MemFileHandle {
    // not optimal,but good enough for now
    fn no_file_err() -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::NotFound, "File was closed")
    }
}

impl std::fmt::Debug for MemFileHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("MemFileHandle")
            .field("inode", &self.inode)
            .finish()
    }
}

impl Read for MemFileHandle {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get_mut(&self.inode)
            .ok_or_else(Self::no_file_err)?;

        file.read(buf)
    }
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let mut inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get_mut(&self.inode)
            .ok_or_else(Self::no_file_err)?;

        file.read_to_end(buf)
    }
    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        let mut inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get_mut(&self.inode)
            .ok_or_else(Self::no_file_err)?;

        file.read_to_string(buf)
    }
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        let mut inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get_mut(&self.inode)
            .ok_or_else(Self::no_file_err)?;

        file.read_exact(buf)
    }
}
impl Seek for MemFileHandle {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        let mut inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get_mut(&self.inode)
            .ok_or_else(Self::no_file_err)?;

        file.seek(pos)
    }
}
impl Write for MemFileHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get_mut(&self.inode)
            .ok_or_else(Self::no_file_err)?;

        file.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        let mut inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get_mut(&self.inode)
            .ok_or_else(Self::no_file_err)?;

        file.flush()
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        let mut inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get_mut(&self.inode)
            .ok_or_else(Self::no_file_err)?;

        file.write_all(buf)
    }
    fn write_fmt(&mut self, fmt: ::std::fmt::Arguments) -> io::Result<()> {
        let mut inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get_mut(&self.inode)
            .ok_or_else(Self::no_file_err)?;

        file.write_fmt(fmt)
    }
}

#[typetag::serde]
impl VirtualFile for MemFileHandle {
    fn last_accessed(&self) -> u64 {
        let inner = self.fs.inner.lock().unwrap();
        inner
            .inodes
            .get(&self.inode)
            .as_ref()
            .map(|file| file.last_accessed())
            .unwrap_or_default()
    }

    fn last_modified(&self) -> u64 {
        let inner = self.fs.inner.lock().unwrap();
        inner
            .inodes
            .get(&self.inode)
            .as_ref()
            .map(|file| file.last_modified())
            .unwrap_or_default()
    }

    fn created_time(&self) -> u64 {
        let inner = self.fs.inner.lock().unwrap();
        inner
            .inodes
            .get(&self.inode)
            .as_ref()
            .map(|file| file.created_time())
            .unwrap_or_default()
    }

    fn size(&self) -> u64 {
        let inner = self.fs.inner.lock().unwrap();
        inner
            .inodes
            .get(&self.inode)
            .as_ref()
            .map(|file| file.size())
            .unwrap_or_default()
    }

    fn set_len(&mut self, new_size: u64) -> Result<(), FsError> {
        let mut inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get_mut(&self.inode)
            .ok_or(FsError::InvalidFd)?;

        file.set_len(new_size)
    }

    fn unlink(&mut self) -> Result<(), FsError> {
        let mut inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get_mut(&self.inode)
            .ok_or(FsError::InvalidFd)?;

        file.unlink()
    }
    fn sync_to_disk(&self) -> Result<(), FsError> {
        let inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get(&self.inode)
            .ok_or_else(Self::no_file_err)?;

        file.sync_to_disk()
    }

    fn rename_file(&self, new_name: &std::path::Path) -> Result<(), FsError> {
        let inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get(&self.inode)
            .ok_or_else(Self::no_file_err)?;

        file.rename_file(new_name)
    }

    fn bytes_available(&self) -> Result<usize, FsError> {
        let inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get(&self.inode)
            .ok_or_else(Self::no_file_err)?;

        file.bytes_available()
    }

    fn get_raw_fd(&self) -> Option<i32> {
        let inner = self.fs.inner.lock().unwrap();
        let file = inner.inodes.get(&self.inode)?;

        file.get_raw_fd()
    }
}

// Stdin / Stdout / Stderr definitions

/// A wrapper type around Stdout that implements `VirtualFile`
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Stdout {
    pub buf: Vec<u8>,
}

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
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        // io::stdout().write(buf)
        unimplemented!();
    }
    fn flush(&mut self) -> io::Result<()> {
        // io::stdout().flush()
        // unimplemented!();
        Ok(())
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        // io::stdout().write_all(buf)
        self.buf.extend_from_slice(&buf);
        Ok(())
    }
    fn write_fmt(&mut self, _fmt: ::std::fmt::Arguments) -> io::Result<()> {
        // io::stdout().write_fmt(fmt)
        unimplemented!();
    }
}

#[typetag::serde]
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
        unimplemented!();
    }

    #[cfg(unix)]
    fn get_raw_fd(&self) -> Option<i32> {
        unimplemented!();
    }

    #[cfg(not(unix))]
    fn get_raw_fd(&self) -> Option<i32> {
        unimplemented!();
    }
}

/// A wrapper type around Stderr that implements `VirtualFile` and
/// `Serialize` + `Deserialize`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Stderr {
    pub buf: Vec<u8>,
}
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
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        // io::stderr().write(buf)
        unimplemented!();
    }
    fn flush(&mut self) -> io::Result<()> {
        // io::stderr().flush()
        // unimplemented!();
        Ok(())
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.buf.extend_from_slice(&buf);
        Ok(())
        // io::stderr().write_all(buf)
        // unimplemented!();
    }
    fn write_fmt(&mut self, _fmt: ::std::fmt::Arguments) -> io::Result<()> {
        // io::stderr().write_fmt(fmt)
        unimplemented!();
    }
}

#[typetag::serde]
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
        unimplemented!();
    }

    #[cfg(unix)]
    fn get_raw_fd(&self) -> Option<i32> {
        unimplemented!();
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
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Stdin {
    pub buf: Vec<u8>,
}

impl Read for Stdin {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let len = std::cmp::min(buf.len(), self.buf.len());
        for (i, val) in self.buf.drain(..len).enumerate() {
            buf[i] = val;
        }
        Ok(len)
        // unimplemented!();
    }
    fn read_to_end(&mut self, _buf: &mut Vec<u8>) -> io::Result<usize> {
        // io::stdin().read_to_end(buf)
        unimplemented!();
    }
    fn read_to_string(&mut self, _buf: &mut String) -> io::Result<usize> {
        // io::stdin().read_to_string(buf)
        unimplemented!();
    }
    fn read_exact(&mut self, _buf: &mut [u8]) -> io::Result<()> {
        // io::stdin().read_exact(buf)
        unimplemented!();
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

#[typetag::serde]
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
        unimplemented!();
    }

    #[cfg(unix)]
    fn get_raw_fd(&self) -> Option<i32> {
        // use std::os::unix::io::AsRawFd;
        // Some(io::stdin().as_raw_fd())
        unimplemented!();
    }

    #[cfg(not(unix))]
    fn get_raw_fd(&self) -> Option<i32> {
        unimplemented!(
            "Stdin::get_raw_fd in VirtualFile is not implemented for non-Unix-like targets yet"
        );
    }
}
