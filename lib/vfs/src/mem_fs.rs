use crate::{
    FileDescriptor, FsError, Metadata, OpenOptions, OpenOptionsConfig, ReadDir, Result, VirtualFile,
};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, Read, Seek, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::debug;

pub type Inode = u64;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
enum Node {
    File {
        name: String,
        inode: Inode,
    },
    Directory {
        name: String,
        children: HashMap<String, Node>,
    },
}

impl Default for Node {
    fn default() -> Self {
        Node::Directory {
            name: "/".to_string(),
            children: Default::default(),
        }
    }
}

#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct FileSystem {
    inner: Arc<Mutex<FileSystemInner>>,
}

#[derive(Default, Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct FileSystemInner {
    // done for recursion purposes
    fs: Node,
    inodes: HashMap<Inode, Box<dyn VirtualFile>>,
    next_inode: Inode,
}

impl FileSystemInner {
    /// Removes a file, returning the `inode` number
    fn remove_file_inner(&mut self, path: &Path) -> Result<Inode> {
        let parent = path.parent().unwrap();
        let file = path.file_name().unwrap();

        let node = self.get_node_at_mut(parent).unwrap();
        let inode = match node {
            Node::Directory { children, .. } => {
                let name = file.to_str().unwrap();
                let inode = match children.get(name).unwrap() {
                    Node::File { inode, .. } => *inode,
                    _ => return Err(FsError::NotAFile),
                };
                children.remove(name);

                inode
            }
            _ => return Err(FsError::NotAFile),
        };

        Ok(inode)
    }

    #[allow(dead_code)]
    fn get_node_at(&self, path: &Path) -> Option<&Node> {
        let mut components = path.components();

        if path.is_absolute() {
            components.next()?;
        }

        let mut node: &Node = &self.fs;

        for component in components {
            match node {
                Node::Directory { children, .. } => {
                    node = children.get(component.as_os_str().to_str().unwrap())?;
                }
                _ => return None,
            }
        }

        Some(node)
    }

    fn get_node_at_mut(&mut self, path: &Path) -> Option<&mut Node> {
        let mut components = path.components();

        if path.is_absolute() {
            components.next()?;
        }

        let mut node: &mut Node = &mut self.fs;

        for component in components {
            match node {
                Node::Directory { children, .. } => {
                    node = children.get_mut(component.as_os_str().to_str().unwrap())?;
                }
                _ => return None,
            }
        }

        Some(node)
    }
}

impl crate::FileSystem for FileSystem {
    fn read_dir(&self, _path: &Path) -> Result<ReadDir> {
        todo!()
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        let parent = path.parent().unwrap();
        let file = path.file_name().unwrap();

        let mut inner = self.inner.lock().unwrap();
        let node = inner.get_node_at_mut(parent).unwrap();

        match node {
            Node::Directory { children, .. } => {
                let name = file.to_str().unwrap();

                if children.contains_key(name) {
                    return Err(FsError::AlreadyExists);
                }

                let directory = Node::Directory {
                    name: name.to_owned(),
                    children: Default::default(),
                };

                children.insert(name.to_owned(), directory);
            }
            _ => return Err(FsError::BaseNotDirectory),
        }

        Ok(())
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        let parent = path.parent().unwrap();
        let file = path.file_name().unwrap();

        let mut inner = self.inner.lock().unwrap();
        let node = inner.get_node_at_mut(parent).unwrap();

        match node {
            Node::Directory { children, .. } => {
                let name = file.to_str().unwrap();

                match children.get(name).unwrap() {
                    Node::Directory { children, .. } => {
                        if !children.is_empty() {
                            return Err(FsError::DirectoryNotEmpty);
                        }
                    }
                    _ => return Err(FsError::BaseNotDirectory),
                }

                children.remove(name);
            }

            _ => return Err(FsError::BaseNotDirectory),
        }

        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let inner = self.inner.lock().unwrap();
        // We assume that we move into a location that has a parent.
        // Otherwise (the root) should not be replaceable, and we should trigger an
        // error.
        let parent_to = to.parent().unwrap();

        // TODO: return a proper error (not generic unknown)
        let node_from = inner.get_node_at(from).ok_or(FsError::UnknownError)?;
        let mut inner = self.inner.lock().unwrap();
        let node_to = inner
            .get_node_at_mut(parent_to)
            .ok_or(FsError::BaseNotDirectory)?;

        // We update the to children of the new dir, adding the old node
        match node_to {
            Node::Directory { children, .. } => {
                let name = to.file_name().unwrap().to_str().unwrap();

                children.insert(name.to_owned(), node_from.clone());
            }
            // If we are trying to move from the root `/dir1` to `/file/dir2`
            _ => return Err(FsError::BaseNotDirectory),
        }

        // We remove the old node location
        match node_from {
            Node::Directory { .. } => {
                self.remove_dir(from)?;
            }
            Node::File { .. } => {
                inner.remove_file_inner(from)?;
            }
        }

        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let inode = inner.remove_file_inner(path)?;
        inner.inodes.remove(&inode).unwrap();

        Ok(())
    }

    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(Box::new(FileOpener(self.clone())))
    }

    fn metadata(&self, _path: &Path) -> Result<Metadata> {
        unimplemented!()
    }
}

#[derive(Clone)]
pub struct FileOpener(FileSystem);

impl crate::FileOpener for FileOpener {
    fn open(&mut self, path: &Path, conf: &OpenOptionsConfig) -> Result<Box<dyn VirtualFile>> {
        // TODO: handle create implying write, etc.
        let read = conf.read();
        let write = conf.write();
        let append = conf.append();
        let virtual_file = Box::new(File::new(vec![], read, write, append)) as Box<dyn VirtualFile>;
        let mut inner = self.0.inner.lock().unwrap();
        let inode = inner.next_inode;

        let parent_path = path.parent().unwrap();
        let file_name = path.file_name().unwrap().to_str().unwrap();
        // TODO: replace with an actual missing directory error
        let parent_node = inner.get_node_at_mut(parent_path).ok_or(FsError::IOError)?;
        match parent_node {
            Node::Directory { children, .. } => {
                if children.contains_key(file_name) {
                    return Err(FsError::AlreadyExists);
                }

                children.insert(
                    file_name.to_owned(),
                    Node::File {
                        name: file_name.to_owned(),
                        inode,
                    },
                );
            }
            _ => {
                // expected directory
                return Err(FsError::BaseNotDirectory);
            }
        }

        inner.next_inode += 1;
        inner.inodes.insert(inode, virtual_file);

        Ok(Box::new(FileHandle {
            fs: self.0.clone(),
            inode,
        }) as Box<dyn VirtualFile>)
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct File {
    buffer: Vec<u8>,
    cursor: usize,
    flags: u16,
    last_accessed: u64,
    last_modified: u64,
    created_time: u64,
}

impl File {
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

impl Read for File {
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
        let s = std::str::from_utf8(&self.buffer[self.cursor..])
            .map_err(|_e| io::ErrorKind::InvalidInput)?;
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

impl Seek for File {
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

impl Write for File {
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

#[cfg_attr(feature = "enable-serde", typetag::serde)]
impl VirtualFile for File {
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

    fn set_len(&mut self, new_size: u64) -> Result<()> {
        self.buffer.resize(new_size as usize, 0);
        Ok(())
    }

    fn unlink(&mut self) -> Result<()> {
        self.buffer.clear();
        self.cursor = 0;
        Ok(())
    }

    fn sync_to_disk(&self) -> Result<()> {
        Ok(())
    }

    fn bytes_available(&self) -> Result<usize> {
        Ok(self.buffer.len() - self.cursor)
    }

    fn get_fd(&self) -> Option<FileDescriptor> {
        None
    }
}

#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct FileHandle {
    // hack, just skip it
    // #[serde(skip)]
    fs: FileSystem,
    inode: u64,
}

impl FileHandle {
    // not optimal,but good enough for now
    fn no_file_err() -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::NotFound, "File was closed")
    }
}

impl std::fmt::Debug for FileHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("FileHandle")
            .field("inode", &self.inode)
            .finish()
    }
}

impl Read for FileHandle {
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

impl Seek for FileHandle {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        let mut inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get_mut(&self.inode)
            .ok_or_else(Self::no_file_err)?;

        file.seek(pos)
    }
}

impl Write for FileHandle {
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

#[cfg_attr(feature = "enable-serde", typetag::serde)]
impl VirtualFile for FileHandle {
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

    fn set_len(&mut self, new_size: u64) -> Result<()> {
        let mut inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get_mut(&self.inode)
            .ok_or(FsError::InvalidFd)?;

        file.set_len(new_size)
    }

    fn unlink(&mut self) -> Result<()> {
        let mut inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get_mut(&self.inode)
            .ok_or(FsError::InvalidFd)?;

        file.unlink()
    }

    fn sync_to_disk(&self) -> Result<()> {
        let inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get(&self.inode)
            .ok_or_else(Self::no_file_err)?;

        file.sync_to_disk()
    }

    fn bytes_available(&self) -> Result<usize> {
        let inner = self.fs.inner.lock().unwrap();
        let file = inner
            .inodes
            .get(&self.inode)
            .ok_or_else(Self::no_file_err)?;

        file.bytes_available()
    }

    fn get_fd(&self) -> Option<FileDescriptor> {
        let inner = self.fs.inner.lock().unwrap();
        let file = inner.inodes.get(&self.inode)?;

        file.get_fd()
    }
}

/// A wrapper type around Stdout that implements `VirtualFile`
#[derive(Debug, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
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

#[cfg_attr(feature = "enable-serde", typetag::serde)]
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
        // unwrap is safe because of get_raw_fd implementation
        unimplemented!();
    }
}

/// A wrapper type around Stderr that implements `VirtualFile` and
/// `Serialize` + `Deserialize`.
#[derive(Debug, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
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

#[cfg_attr(feature = "enable-serde", typetag::serde)]
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
        unimplemented!();
    }
}

/// A wrapper type around Stdin that implements `VirtualFile` and
/// `Serialize` + `Deserialize`.
#[derive(Debug, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
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

#[cfg_attr(feature = "enable-serde", typetag::serde)]
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
        unimplemented!();
    }
}
