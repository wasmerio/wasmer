//! This module contains the `FileHandle` and `File`
//! implementations. They aren't exposed to the public API. Only
//! `FileHandle` can be used through the `VirtualFile` trait object.

use super::*;
use crate::{FileDescriptor, FsError, Result, VirtualFile};
use std::cmp;
use std::convert::TryInto;
use std::fmt;
use std::io::{self, Read, Seek, Write};
use std::str;

/// A file handle. The file system doesn't return the [`File`] type
/// directly, but rather this `FileHandle` type, which contains the
/// inode, the flags, and (a light copy of) the filesystem. For each
/// operations, it is checked that the permissions allow the
/// operations to be executed, and then it is checked that the file
/// still exists in the file system. After that, the operation is
/// delegated to the file itself.
#[derive(Clone)]
pub(super) struct FileHandle {
    inode: Inode,
    filesystem: FileSystem,
    readable: bool,
    writable: bool,
    append_mode: bool,
}

impl FileHandle {
    pub(super) fn new(
        inode: Inode,
        filesystem: FileSystem,
        readable: bool,
        writable: bool,
        append_mode: bool,
    ) -> Self {
        Self {
            inode,
            filesystem,
            readable,
            writable,
            append_mode,
        }
    }
}

impl VirtualFile for FileHandle {
    fn last_accessed(&self) -> u64 {
        let fs = match self.filesystem.inner.try_read() {
            Ok(fs) => fs,
            _ => return 0,
        };

        let inode = fs.storage.get(self.inode);
        match inode {
            Some(node) => node.metadata().accessed,
            _ => 0,
        }
    }

    fn last_modified(&self) -> u64 {
        let fs = match self.filesystem.inner.try_read() {
            Ok(fs) => fs,
            _ => return 0,
        };

        let inode = fs.storage.get(self.inode);
        match inode {
            Some(node) => node.metadata().modified,
            _ => 0,
        }
    }

    fn created_time(&self) -> u64 {
        let fs = match self.filesystem.inner.try_read() {
            Ok(fs) => fs,
            _ => return 0,
        };

        let inode = fs.storage.get(self.inode);
        let node = match inode {
            Some(node) => node,
            _ => return 0,
        };

        node.metadata().created
    }

    fn size(&self) -> u64 {
        let fs = match self.filesystem.inner.try_read() {
            Ok(fs) => fs,
            _ => return 0,
        };

        let inode = fs.storage.get(self.inode);
        match inode {
            Some(Node::File { file, .. }) => file.len().try_into().unwrap_or(0),
            _ => 0,
        }
    }

    fn set_len(&mut self, new_size: u64) -> Result<()> {
        let mut fs = self
            .filesystem
            .inner
            .try_write()
            .map_err(|_| FsError::Lock)?;

        let inode = fs.storage.get_mut(self.inode);
        match inode {
            Some(Node::File { file, metadata, .. }) => {
                file.buffer
                    .resize(new_size.try_into().map_err(|_| FsError::UnknownError)?, 0);
                metadata.len = new_size;
            }
            _ => return Err(FsError::NotAFile),
        }

        Ok(())
    }

    fn unlink(&mut self) -> Result<()> {
        let (inode_of_parent, position, inode_of_file) = {
            // Read lock.
            let fs = self
                .filesystem
                .inner
                .try_read()
                .map_err(|_| FsError::Lock)?;

            // The inode of the file.
            let inode_of_file = self.inode;

            // Find the position of the file in the parent, and the
            // inode of the parent.
            let (position, inode_of_parent) = fs
                .storage
                .iter()
                .find_map(|(inode_of_parent, node)| match node {
                    Node::Directory { children, .. } => {
                        children.iter().enumerate().find_map(|(nth, inode)| {
                            if inode == &inode_of_file {
                                Some((nth, inode_of_parent))
                            } else {
                                None
                            }
                        })
                    }

                    _ => None,
                })
                .ok_or(FsError::BaseNotDirectory)?;

            (inode_of_parent, position, inode_of_file)
        };

        {
            // Write lock.
            let mut fs = self
                .filesystem
                .inner
                .try_write()
                .map_err(|_| FsError::Lock)?;

            // Remove the file from the storage.
            fs.storage.remove(inode_of_file);

            // Remove the child from the parent directory.
            fs.remove_child_from_node(inode_of_parent, position)?;
        }

        Ok(())
    }

    fn bytes_available(&self) -> Result<usize> {
        let fs = self
            .filesystem
            .inner
            .try_read()
            .map_err(|_| FsError::Lock)?;

        let inode = fs.storage.get(self.inode);
        match inode {
            Some(Node::File { file, .. }) => Ok(file.buffer.len() - file.cursor),
            _ => Err(FsError::NotAFile),
        }
    }

    fn get_fd(&self) -> Option<FileDescriptor> {
        Some(FileDescriptor(self.inode))
    }
}

#[cfg(test)]
mod test_virtual_file {
    use crate::{mem_fs::*, FileDescriptor, FileSystem as FS};
    use std::thread::sleep;
    use std::time::Duration;

    macro_rules! path {
        ($path:expr) => {
            std::path::Path::new($path)
        };
    }

    #[test]
    fn test_last_accessed() {
        let fs = FileSystem::default();

        let file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");
        let last_accessed_time = file.last_accessed();

        assert!(last_accessed_time > 0, "last accessed time is not zero");

        sleep(Duration::from_secs(3));

        let file = fs
            .new_open_options()
            .read(true)
            .open(path!("/foo.txt"))
            .expect("failed to open a file");
        let next_last_accessed_time = file.last_accessed();

        assert!(
            next_last_accessed_time > last_accessed_time,
            "the last accessed time is updated"
        );
    }

    #[test]
    fn test_last_modified() {
        let fs = FileSystem::default();

        let file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(file.last_modified() > 0, "last modified time is not zero");
    }

    #[test]
    fn test_created_time() {
        let fs = FileSystem::default();

        let file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");
        let created_time = file.created_time();

        assert!(created_time > 0, "created time is not zero");

        let file = fs
            .new_open_options()
            .read(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");
        let next_created_time = file.created_time();

        assert_eq!(
            next_created_time, created_time,
            "created time stays constant"
        );
    }

    #[test]
    fn test_size() {
        let fs = FileSystem::default();

        let file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert_eq!(file.size(), 0, "new file is empty");
    }

    #[test]
    fn test_set_len() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(matches!(file.set_len(7), Ok(())), "setting a new length");
        assert_eq!(file.size(), 7, "file has a new length");
    }

    #[test]
    fn test_unlink() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        {
            let fs_inner = fs.inner.read().unwrap();

            assert_eq!(fs_inner.storage.len(), 2, "storage has the new file");
            assert!(
                matches!(
                    fs_inner.storage.get(ROOT_INODE),
                    Some(Node::Directory {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    }) if name == "/" && children == &[1]
                ),
                "`/` contains `foo.txt`",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(1),
                    Some(Node::File {
                        inode: 1,
                        name,
                        ..
                    }) if name == "foo.txt"
                ),
                "`foo.txt` exists and is a file",
            );
        }

        assert_eq!(file.unlink(), Ok(()), "unlinking the file");

        {
            let fs_inner = fs.inner.read().unwrap();

            assert_eq!(
                fs_inner.storage.len(),
                1,
                "storage no longer has the new file"
            );
            assert!(
                matches!(
                    fs_inner.storage.get(ROOT_INODE),
                    Some(Node::Directory {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    }) if name == "/" && children.is_empty()
                ),
                "`/` is empty",
            );
        }
    }

    #[test]
    fn test_bytes_available() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert_eq!(file.bytes_available(), Ok(0), "zero bytes available");
        assert_eq!(file.set_len(7), Ok(()), "resizing the file");
        assert_eq!(file.bytes_available(), Ok(7), "seven bytes available");
    }

    #[test]
    fn test_fd() {
        let fs = FileSystem::default();

        let file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.get_fd(), Some(FileDescriptor(1))),
            "reading the file descriptor",
        );
    }
}

impl Read for FileHandle {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.readable {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "the file (inode `{}) doesn't have the `read` permission",
                    self.inode
                ),
            ));
        }

        let mut fs =
            self.filesystem.inner.try_write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

        let inode = fs.storage.get_mut(self.inode);
        let file = match inode {
            Some(Node::File { file, .. }) => file,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("inode `{}` doesn't match a file", self.inode),
                ))
            }
        };

        file.read(buf)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        if !self.readable {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "the file (inode `{}) doesn't have the `read` permission",
                    self.inode
                ),
            ));
        }

        let mut fs =
            self.filesystem.inner.try_write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

        let inode = fs.storage.get_mut(self.inode);
        let file = match inode {
            Some(Node::File { file, .. }) => file,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("inode `{}` doesn't match a file", self.inode),
                ))
            }
        };

        file.read_to_end(buf)
    }

    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        // SAFETY: `String::as_mut_vec` cannot check that modifcations
        // of the `Vec` will produce a valid UTF-8 string. In our
        // case, we use `str::from_utf8` to ensure that the UTF-8
        // constraint still hold before returning.
        let bytes_buffer = unsafe { buf.as_mut_vec() };
        bytes_buffer.clear();
        let read = self.read_to_end(bytes_buffer)?;

        if str::from_utf8(bytes_buffer).is_err() {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "buffer did not contain valid UTF-8",
            ))
        } else {
            Ok(read)
        }
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        if !self.readable {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "the file (inode `{}) doesn't have the `read` permission",
                    self.inode
                ),
            ));
        }

        let mut fs =
            self.filesystem.inner.try_write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

        let inode = fs.storage.get_mut(self.inode);
        let file = match inode {
            Some(Node::File { file, .. }) => file,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("inode `{}` doesn't match a file", self.inode),
                ))
            }
        };

        file.read_exact(buf)
    }
}

impl Seek for FileHandle {
    fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
        // In `append` mode, it's not possible to seek in the file. In
        // [`open(2)`](https://man7.org/linux/man-pages/man2/open.2.html),
        // the `O_APPEND` option describes this behavior well:
        //
        // > Before each write(2), the file offset is positioned at
        // > the end of the file, as if with lseek(2).  The
        // > modification of the file offset and the write operation
        // > are performed as a single atomic step.
        // >
        // > O_APPEND may lead to corrupted files on NFS filesystems
        // > if more than one process appends data to a file at once.
        // > This is because NFS does not support appending to a file,
        // > so the client kernel has to simulate it, which can't be
        // > done without a race condition.
        if self.append_mode {
            return Ok(0);
        }

        let mut fs =
            self.filesystem.inner.try_write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

        let inode = fs.storage.get_mut(self.inode);
        let file = match inode {
            Some(Node::File { file, .. }) => file,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("inode `{}` doesn't match a file", self.inode),
                ))
            }
        };

        file.seek(position)
    }
}

impl Write for FileHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.writable {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "the file (inode `{}) doesn't have the `write` permission",
                    self.inode
                ),
            ));
        }

        let mut fs =
            self.filesystem.inner.try_write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

        let inode = fs.storage.get_mut(self.inode);
        let (file, metadata) = match inode {
            Some(Node::File { file, metadata, .. }) => (file, metadata),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("inode `{}` doesn't match a file", self.inode),
                ))
            }
        };

        let bytes_written = file.write(buf)?;

        metadata.len = file.len().try_into().unwrap();

        Ok(bytes_written)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    #[allow(clippy::unused_io_amount)]
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.write(buf)?;

        Ok(())
    }
}

#[cfg(test)]
mod test_read_write_seek {
    use crate::{mem_fs::*, FileSystem as FS};
    use std::io;

    macro_rules! path {
        ($path:expr) => {
            std::path::Path::new($path)
        };
    }

    #[test]
    fn test_writing_at_various_positions() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.write(b"foo"), Ok(3)),
            "writing `foo` at the end of the file",
        );
        assert_eq!(file.size(), 3, "checking the size of the file");

        assert!(
            matches!(file.write(b"bar"), Ok(3)),
            "writing `bar` at the end of the file",
        );
        assert_eq!(file.size(), 6, "checking the size of the file");

        assert!(
            matches!(file.seek(io::SeekFrom::Start(0)), Ok(0)),
            "seeking to 0",
        );

        assert!(
            matches!(file.write(b"baz"), Ok(3)),
            "writing `baz` at the beginning of the file",
        );
        assert_eq!(file.size(), 9, "checking the size of the file");

        assert!(
            matches!(file.write(b"qux"), Ok(3)),
            "writing `qux` in the middle of the file",
        );
        assert_eq!(file.size(), 12, "checking the size of the file");

        assert!(
            matches!(file.seek(io::SeekFrom::Start(0)), Ok(0)),
            "seeking to 0",
        );

        let mut string = String::new();
        assert!(
            matches!(file.read_to_string(&mut string), Ok(12)),
            "reading `bazquxfoobar`",
        );
        assert_eq!(string, "bazquxfoobar");

        assert!(
            matches!(file.seek(io::SeekFrom::Current(-6)), Ok(6)),
            "seeking to 6",
        );

        let mut string = String::new();
        assert!(
            matches!(file.read_to_string(&mut string), Ok(6)),
            "reading `foobar`",
        );
        assert_eq!(string, "foobar");

        assert!(
            matches!(file.seek(io::SeekFrom::End(0)), Ok(12)),
            "seeking to 12",
        );

        let mut string = String::new();
        assert!(
            matches!(file.read_to_string(&mut string), Ok(0)),
            "reading ``",
        );
        assert_eq!(string, "");
    }

    #[test]
    fn test_reading() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(fs.metadata(path!("/foo.txt")), Ok(Metadata { len: 0, .. })),
            "checking the `metadata.len` is 0",
        );
        assert!(
            matches!(file.write(b"foobarbazqux"), Ok(12)),
            "writing `foobarbazqux`",
        );

        assert!(
            matches!(file.seek(io::SeekFrom::Start(0)), Ok(0)),
            "seeking to 0",
        );

        let mut buffer = [0; 6];
        assert!(
            matches!(file.read(&mut buffer[..]), Ok(6)),
            "reading 6 bytes",
        );
        assert_eq!(buffer, b"foobar"[..], "checking the 6 bytes");

        assert!(
            matches!(file.seek(io::SeekFrom::Start(0)), Ok(0)),
            "seeking to 0",
        );

        let mut buffer = [0; 16];
        assert!(
            matches!(file.read(&mut buffer[..]), Ok(12)),
            "reading more bytes than available",
        );
        assert_eq!(buffer[..12], b"foobarbazqux"[..], "checking the 12 bytes");
        assert!(
            matches!(fs.metadata(path!("/foo.txt")), Ok(Metadata { len: 12, .. })),
            "checking the `metadata.len` is 0",
        );
    }

    #[test]
    fn test_reading_to_the_end() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.write(b"foobarbazqux"), Ok(12)),
            "writing `foobarbazqux`",
        );

        assert!(
            matches!(file.seek(io::SeekFrom::Start(0)), Ok(0)),
            "seeking to 0",
        );

        let mut buffer = Vec::new();
        assert!(
            matches!(file.read_to_end(&mut buffer), Ok(12)),
            "reading all bytes",
        );
        assert_eq!(buffer, b"foobarbazqux"[..], "checking all the bytes");
    }

    #[test]
    fn test_reading_to_string() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.write(b"foobarbazqux"), Ok(12)),
            "writing `foobarbazqux`",
        );

        assert!(
            matches!(file.seek(io::SeekFrom::Start(6)), Ok(6)),
            "seeking to 0",
        );

        let mut string = String::new();
        assert!(
            matches!(file.read_to_string(&mut string), Ok(6)),
            "reading a string",
        );
        assert_eq!(string, "bazqux", "checking the string");
    }

    #[test]
    fn test_reading_exact_buffer() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.write(b"foobarbazqux"), Ok(12)),
            "writing `foobarbazqux`",
        );

        assert!(
            matches!(file.seek(io::SeekFrom::Start(6)), Ok(6)),
            "seeking to 0",
        );

        let mut buffer = [0; 16];
        assert!(
            matches!(file.read_exact(&mut buffer), Err(_)),
            "failing to read an exact buffer",
        );

        assert!(
            matches!(file.seek(io::SeekFrom::End(-5)), Ok(7)),
            "seeking to 7",
        );

        let mut buffer = [0; 3];
        assert!(
            matches!(file.read_exact(&mut buffer), Ok(())),
            "failing to read an exact buffer",
        );
    }
}

impl fmt::Debug for FileHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileHandle")
            .field("inode", &self.inode)
            .finish()
    }
}

/// The real file! It is simply a buffer of bytes with a cursor that
/// represents a read/write position in the buffer.
#[derive(Debug)]
pub(super) struct File {
    buffer: Vec<u8>,
    cursor: usize,
}

impl File {
    pub(super) fn new() -> Self {
        Self {
            buffer: Vec::new(),
            cursor: 0,
        }
    }

    pub(super) fn truncate(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
    }

    pub(super) fn len(&self) -> usize {
        self.buffer.len()
    }
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let max_to_read = cmp::min(self.buffer.len() - self.cursor, buf.len());
        let data_to_copy = &self.buffer[self.cursor..][..max_to_read];

        // SAFETY: `buf[..max_to_read]` and `data_to_copy` have the same size, due to
        // how `max_to_read` is computed.
        buf[..max_to_read].copy_from_slice(data_to_copy);

        self.cursor += max_to_read;

        Ok(max_to_read)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let data_to_copy = &self.buffer[self.cursor..];
        let max_to_read = data_to_copy.len();

        // `buf` is too small to contain the data. Let's resize it.
        if max_to_read > buf.len() {
            // Let's resize the capacity if needed.
            if max_to_read > buf.capacity() {
                buf.reserve_exact(max_to_read - buf.capacity());
            }

            // SAFETY: The space is reserved, and it's going to be
            // filled with `copy_from_slice` below.
            unsafe { buf.set_len(max_to_read) }
        }

        // SAFETY: `buf` and `data_to_copy` have the same size, see
        // above.
        buf.copy_from_slice(data_to_copy);

        self.cursor += max_to_read;

        Ok(max_to_read)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        if buf.len() > (self.buffer.len() - self.cursor) {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "not enough data available in file",
            ));
        }

        let max_to_read = cmp::min(buf.len(), self.buffer.len() - self.cursor);
        let data_to_copy = &self.buffer[self.cursor..][..max_to_read];

        // SAFETY: `buf` and `data_to_copy` have the same size.
        buf.copy_from_slice(data_to_copy);

        self.cursor += data_to_copy.len();

        Ok(())
    }
}

impl Seek for File {
    fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
        let to_err = |_| io::ErrorKind::InvalidInput;

        // Calculate the next cursor.
        let next_cursor: i64 = match position {
            // Calculate from the beginning, so `0 + offset`.
            io::SeekFrom::Start(offset) => offset.try_into().map_err(to_err)?,

            // Calculate from the end, so `buffer.len() + offset`.
            io::SeekFrom::End(offset) => {
                TryInto::<i64>::try_into(self.buffer.len()).map_err(to_err)? + offset
            }

            // Calculate from the current cursor, so `cursor + offset`.
            io::SeekFrom::Current(offset) => {
                TryInto::<i64>::try_into(self.cursor).map_err(to_err)? + offset
            }
        };

        // It's an error to seek before byte 0.
        if next_cursor < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seeking before the byte 0",
            ));
        }

        // In this implementation, it's an error to seek beyond the
        // end of the buffer.
        self.cursor = cmp::min(self.buffer.len(), next_cursor.try_into().map_err(to_err)?);

        Ok(self.cursor.try_into().map_err(to_err)?)
    }
}

impl Write for File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.cursor {
            // The cursor is at the end of the buffer: happy path!
            position if position == self.buffer.len() => {
                self.buffer.extend_from_slice(buf);
            }

            // The cursor is at the beginning of the buffer (and the
            // buffer is not empty, otherwise it would have been
            // caught by the previous arm): almost a happy path!
            0 => {
                let mut new_buffer = Vec::with_capacity(self.buffer.len() + buf.len());
                new_buffer.extend_from_slice(buf);
                new_buffer.append(&mut self.buffer);

                self.buffer = new_buffer;
            }

            // The cursor is somewhere in the buffer: not the happy path.
            position => {
                self.buffer.reserve_exact(buf.len());

                let mut remainder = self.buffer.split_off(position);
                self.buffer.extend_from_slice(buf);
                self.buffer.append(&mut remainder);
            }
        }

        self.cursor += buf.len();

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
