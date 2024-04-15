//! This module contains the `FileHandle` and `File`
//! implementations. They aren't exposed to the public API. Only
//! `FileHandle` can be used through the `VirtualFile` trait object.

use futures::future::BoxFuture;
use tokio::io::AsyncRead;
use tokio::io::{AsyncSeek, AsyncWrite};

use self::offloaded_file::OffloadWrite;

use super::*;
use crate::limiter::TrackedVec;
use crate::{CopyOnWriteFile, FsError, Result, VirtualFile};
use std::borrow::Cow;
use std::cmp;
use std::convert::TryInto;
use std::fmt;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A file handle. The file system doesn't return the [`File`] type
/// directly, but rather this `FileHandle` type, which contains the
/// inode, the flags, and (a light copy of) the filesystem. For each
/// operations, it is checked that the permissions allow the
/// operations to be executed, and then it is checked that the file
/// still exists in the file system. After that, the operation is
/// delegated to the file itself.
pub(super) struct FileHandle {
    inode: Inode,
    filesystem: FileSystem,
    readable: bool,
    writable: bool,
    append_mode: bool,
    cursor: u64,
    arc_file: Option<Result<Box<dyn VirtualFile + Send + Sync + 'static>>>,
}

impl Clone for FileHandle {
    fn clone(&self) -> Self {
        Self {
            inode: self.inode,
            filesystem: self.filesystem.clone(),
            readable: self.readable,
            writable: self.writable,
            append_mode: self.append_mode,
            cursor: self.cursor,
            arc_file: None,
        }
    }
}

impl FileHandle {
    pub(super) fn new(
        inode: Inode,
        filesystem: FileSystem,
        readable: bool,
        writable: bool,
        append_mode: bool,
        cursor: u64,
    ) -> Self {
        Self {
            inode,
            filesystem,
            readable,
            writable,
            append_mode,
            cursor,
            arc_file: None,
        }
    }

    fn lazy_load_arc_file_mut(&mut self) -> Result<&mut dyn VirtualFile> {
        if self.arc_file.is_none() {
            let fs = match self.filesystem.inner.read() {
                Ok(fs) => fs,
                _ => return Err(FsError::EntryNotFound),
            };

            let inode = fs.storage.get(self.inode);
            match inode {
                Some(Node::ArcFile(node)) => {
                    self.arc_file.replace(
                        node.fs
                            .new_open_options()
                            .read(self.readable)
                            .write(self.writable)
                            .append(self.append_mode)
                            .open(node.path.as_path()),
                    );
                }
                _ => return Err(FsError::EntryNotFound),
            }
        }
        Ok(self
            .arc_file
            .as_mut()
            .unwrap()
            .as_mut()
            .map_err(|err| *err)?
            .as_mut())
    }
}

impl VirtualFile for FileHandle {
    fn last_accessed(&self) -> u64 {
        let fs = match self.filesystem.inner.read() {
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
        let fs = match self.filesystem.inner.read() {
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
        let fs = match self.filesystem.inner.read() {
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
        let fs = match self.filesystem.inner.read() {
            Ok(fs) => fs,
            _ => return 0,
        };

        let inode = fs.storage.get(self.inode);
        match inode {
            Some(Node::File(node)) => node.file.len().try_into().unwrap_or(0),
            Some(Node::OffloadedFile(node)) => node.file.len(),
            Some(Node::ReadOnlyFile(node)) => node.file.len().try_into().unwrap_or(0),
            Some(Node::CustomFile(node)) => {
                let file = node.file.lock().unwrap();
                file.size()
            }
            Some(Node::ArcFile(node)) => match self.arc_file.as_ref() {
                Some(file) => file.as_ref().map(|file| file.size()).unwrap_or(0),
                None => node
                    .fs
                    .new_open_options()
                    .read(self.readable)
                    .write(self.writable)
                    .append(self.append_mode)
                    .open(node.path.as_path())
                    .map(|file| file.size())
                    .unwrap_or(0),
            },
            _ => 0,
        }
    }

    fn set_len(&mut self, new_size: u64) -> Result<()> {
        let mut fs = self.filesystem.inner.write().map_err(|_| FsError::Lock)?;

        let inode = fs.storage.get_mut(self.inode);
        match inode {
            Some(Node::File(FileNode { file, metadata, .. })) => {
                file.buffer
                    .resize(new_size.try_into().map_err(|_| FsError::UnknownError)?, 0)?;
                metadata.len = new_size;
            }
            Some(Node::OffloadedFile(OffloadedFileNode { file, metadata, .. })) => {
                file.resize(new_size, 0);
                metadata.len = new_size;
            }
            Some(Node::CustomFile(node)) => {
                let mut file = node.file.lock().unwrap();
                file.set_len(new_size)?;
                node.metadata.len = new_size;
            }
            Some(Node::ReadOnlyFile { .. }) => return Err(FsError::PermissionDenied),
            Some(Node::ArcFile { .. }) => {
                drop(fs);
                let file = self.lazy_load_arc_file_mut()?;
                file.set_len(new_size)?;
            }
            _ => return Err(FsError::NotAFile),
        }

        Ok(())
    }

    fn unlink(&mut self) -> Result<()> {
        let filesystem = self.filesystem.clone();
        let inode = self.inode;

        let (inode_of_parent, position, inode_of_file) = {
            // Read lock.
            let fs = filesystem.inner.read().map_err(|_| FsError::Lock)?;

            // The inode of the file.
            let inode_of_file = inode;

            // Find the position of the file in the parent, and the
            // inode of the parent.
            let (position, inode_of_parent) = fs
                .storage
                .iter()
                .find_map(|(inode_of_parent, node)| match node {
                    Node::Directory(DirectoryNode { children, .. }) => {
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
            let mut fs = filesystem.inner.write().map_err(|_| FsError::Lock)?;

            // Remove the file from the storage.
            fs.storage.remove(inode_of_file);

            // Remove the child from the parent directory.
            fs.remove_child_from_node(inode_of_parent, position)?;
        }

        Ok(())
    }

    fn get_special_fd(&self) -> Option<u32> {
        let fs = match self.filesystem.inner.read() {
            Ok(a) => a,
            Err(_) => {
                return None;
            }
        };

        let inode = fs.storage.get(self.inode);
        match inode {
            Some(Node::CustomFile(node)) => {
                let file = node.file.lock().unwrap();
                file.get_special_fd()
            }
            Some(Node::ArcFile(node)) => match self.arc_file.as_ref() {
                Some(file) => file
                    .as_ref()
                    .map(|file| file.get_special_fd())
                    .unwrap_or(None),
                None => node
                    .fs
                    .new_open_options()
                    .read(self.readable)
                    .write(self.writable)
                    .append(self.append_mode)
                    .open(node.path.as_path())
                    .map(|file| file.get_special_fd())
                    .unwrap_or(None),
            },
            _ => None,
        }
    }

    fn copy_reference(
        &mut self,
        src: Box<dyn VirtualFile + Send + Sync + 'static>,
    ) -> BoxFuture<'_, std::io::Result<()>> {
        let inner = self.filesystem.inner.clone();
        Box::pin(async move {
            let mut fs = inner.write().unwrap();
            let inode = fs.storage.get_mut(self.inode);
            match inode {
                Some(inode) => {
                    let metadata = Metadata {
                        ft: crate::FileType {
                            file: true,
                            ..Default::default()
                        },
                        accessed: src.last_accessed(),
                        created: src.created_time(),
                        modified: src.last_modified(),
                        len: src.size(),
                    };

                    *inode = Node::CustomFile(CustomFileNode {
                        inode: inode.inode(),
                        name: inode.name().to_string_lossy().to_string().into(),
                        file: Mutex::new(Box::new(CopyOnWriteFile::new(src))),
                        metadata,
                    });
                    Ok(())
                }
                None => Err(std::io::ErrorKind::InvalidInput.into()),
            }
        })
    }

    fn poll_read_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        if !self.readable {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "the file (inode `{}) doesn't have the `read` permission",
                    self.inode
                ),
            )));
        }

        let mut fs =
            self.filesystem.inner.write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

        let inode = fs.storage.get_mut(self.inode);
        match inode {
            Some(Node::File(node)) => {
                let remaining = node.file.buffer.len() - (self.cursor as usize);
                Poll::Ready(Ok(remaining))
            }
            Some(Node::OffloadedFile(node)) => {
                let remaining = node.file.len() as usize - (self.cursor as usize);
                Poll::Ready(Ok(remaining))
            }
            Some(Node::ReadOnlyFile(node)) => {
                let remaining = node.file.buffer.len() - (self.cursor as usize);
                Poll::Ready(Ok(remaining))
            }
            Some(Node::CustomFile(node)) => {
                let mut file = node.file.lock().unwrap();
                let file = Pin::new(file.as_mut());
                file.poll_read_ready(cx)
            }
            Some(Node::ArcFile(_)) => {
                drop(fs);
                match self.lazy_load_arc_file_mut() {
                    Ok(file) => {
                        let file = Pin::new(file);
                        file.poll_read_ready(cx)
                    }
                    Err(_) => Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("inode `{}` doesn't match a file", self.inode),
                    ))),
                }
            }
            _ => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("inode `{}` doesn't match a file", self.inode),
            ))),
        }
    }

    fn poll_write_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        if !self.readable {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "the file (inode `{}) doesn't have the `read` permission",
                    self.inode
                ),
            )));
        }

        let mut fs =
            self.filesystem.inner.write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

        let inode = fs.storage.get_mut(self.inode);
        match inode {
            Some(Node::File(_)) => Poll::Ready(Ok(8192)),
            Some(Node::OffloadedFile(_)) => Poll::Ready(Ok(8192)),
            Some(Node::ReadOnlyFile(_)) => Poll::Ready(Ok(0)),
            Some(Node::CustomFile(node)) => {
                let mut file = node.file.lock().unwrap();
                let file = Pin::new(file.as_mut());
                file.poll_read_ready(cx)
            }
            Some(Node::ArcFile(_)) => {
                drop(fs);
                match self.lazy_load_arc_file_mut() {
                    Ok(file) => {
                        let file = Pin::new(file);
                        file.poll_read_ready(cx)
                    }
                    Err(_) => Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("inode `{}` doesn't match a file", self.inode),
                    ))),
                }
            }
            _ => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("inode `{}` doesn't match a file", self.inode),
            ))),
        }
    }

    fn write_from_mmap(&mut self, offset: u64, size: u64) -> std::io::Result<()> {
        if !self.writable {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "the file (inode `{}) doesn't have the `write` permission",
                    self.inode
                ),
            ));
        }

        let mut cursor = self.cursor;
        {
            let mut fs = self.filesystem.inner.write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

            let inode = fs.storage.get_mut(self.inode);
            match inode {
                Some(Node::OffloadedFile(node)) => {
                    node.file
                        .write(OffloadWrite::MmapOffset { offset, size }, &mut cursor)?;
                    node.metadata.len = node.file.len();
                }
                _ => {
                    return Err(io::ErrorKind::Unsupported.into());
                }
            }
        }
        self.cursor = cursor;
        Ok(())
    }
}

#[cfg(test)]
mod test_virtual_file {
    use crate::{mem_fs::*, FileSystem as FS};
    use std::thread::sleep;
    use std::time::Duration;

    macro_rules! path {
        ($path:expr) => {
            std::path::Path::new($path)
        };
    }

    #[tokio::test]
    async fn test_last_accessed() {
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

    #[tokio::test]
    async fn test_last_modified() {
        let fs = FileSystem::default();

        let file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(file.last_modified() > 0, "last modified time is not zero");
    }

    #[tokio::test]
    async fn test_created_time() {
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

    #[tokio::test]
    async fn test_size() {
        let fs = FileSystem::default();

        let file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert_eq!(file.size(), 0, "new file is empty");
    }

    #[tokio::test]
    async fn test_set_len() {
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

    #[tokio::test]
    async fn test_unlink() {
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
                    Some(Node::Directory(DirectoryNode {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    })) if name == "/" && children == &[1]
                ),
                "`/` contains `foo.txt`",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(1),
                    Some(Node::File(FileNode {
                        inode: 1,
                        name,
                        ..
                    })) if name == "foo.txt"
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
                    Some(Node::Directory(DirectoryNode {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    })) if name == "/" && children.is_empty()
                ),
                "`/` is empty",
            );
        }
    }
}

impl AsyncRead for FileHandle {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.readable {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "the file (inode `{}) doesn't have the `read` permission",
                    self.inode
                ),
            )));
        }

        let mut cursor = self.cursor;
        let ret = {
            let mut fs = self.filesystem.inner.write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

            let inode = fs.storage.get_mut(self.inode);
            match inode {
                Some(Node::File(node)) => {
                    let read = unsafe {
                        node.file
                            .read(std::mem::transmute(buf.unfilled_mut()), &mut cursor)
                    };
                    if let Ok(read) = &read {
                        unsafe { buf.assume_init(*read) };
                        buf.advance(*read);
                    }
                    Poll::Ready(read.map(|_| ()))
                }
                Some(Node::OffloadedFile(node)) => {
                    let read = unsafe {
                        node.file
                            .read(std::mem::transmute(buf.unfilled_mut()), &mut cursor)
                    };
                    if let Ok(read) = &read {
                        unsafe { buf.assume_init(*read) };
                        buf.advance(*read);
                    }
                    Poll::Ready(read.map(|_| ()))
                }
                Some(Node::ReadOnlyFile(node)) => {
                    let read = unsafe {
                        node.file
                            .read(std::mem::transmute(buf.unfilled_mut()), &mut cursor)
                    };
                    if let Ok(read) = &read {
                        unsafe { buf.assume_init(*read) };
                        buf.advance(*read);
                    }
                    Poll::Ready(read.map(|_| ()))
                }
                Some(Node::CustomFile(node)) => {
                    let mut file = node.file.lock().unwrap();
                    let file = Pin::new(file.as_mut());
                    file.poll_read(cx, buf)
                }
                Some(Node::ArcFile(_)) => {
                    drop(fs);
                    match self.lazy_load_arc_file_mut() {
                        Ok(file) => {
                            let file = Pin::new(file);
                            file.poll_read(cx, buf)
                        }
                        Err(_) => {
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::NotFound,
                                format!("inode `{}` doesn't match a file", self.inode),
                            )))
                        }
                    }
                }
                _ => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("inode `{}` doesn't match a file", self.inode),
                    )));
                }
            }
        };
        self.cursor = cursor;
        ret
    }
}

impl AsyncSeek for FileHandle {
    fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        if self.append_mode {
            return Ok(());
        }

        let mut cursor = self.cursor;
        let ret = {
            let mut fs = self.filesystem.inner.write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

            let inode = fs.storage.get_mut(self.inode);
            match inode {
                Some(Node::File(node)) => {
                    node.file.seek(position, &mut cursor)?;
                    Ok(())
                }
                Some(Node::OffloadedFile(node)) => {
                    node.file.seek(position, &mut cursor)?;
                    Ok(())
                }
                Some(Node::ReadOnlyFile(node)) => {
                    node.file.seek(position, &mut cursor)?;
                    Ok(())
                }
                Some(Node::CustomFile(node)) => {
                    let mut file = node.file.lock().unwrap();
                    let file = Pin::new(file.as_mut());
                    file.start_seek(position)
                }
                Some(Node::ArcFile(_)) => {
                    drop(fs);
                    match self.lazy_load_arc_file_mut() {
                        Ok(file) => {
                            let file = Pin::new(file);
                            file.start_seek(position)
                        }
                        Err(_) => {
                            return Err(io::Error::new(
                                io::ErrorKind::NotFound,
                                format!("inode `{}` doesn't match a file", self.inode),
                            ));
                        }
                    }
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("inode `{}` doesn't match a file", self.inode),
                    ));
                }
            }
        };
        self.cursor = cursor;
        ret
    }

    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
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
            return Poll::Ready(Ok(0));
        }

        let mut fs =
            self.filesystem.inner.write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

        let inode = fs.storage.get_mut(self.inode);
        match inode {
            Some(Node::File { .. }) => Poll::Ready(Ok(self.cursor)),
            Some(Node::OffloadedFile { .. }) => Poll::Ready(Ok(self.cursor)),
            Some(Node::ReadOnlyFile { .. }) => Poll::Ready(Ok(self.cursor)),
            Some(Node::CustomFile(node)) => {
                let mut file = node.file.lock().unwrap();
                let file = Pin::new(file.as_mut());
                file.poll_complete(cx)
            }
            Some(Node::ArcFile { .. }) => {
                drop(fs);
                match self.lazy_load_arc_file_mut() {
                    Ok(file) => {
                        let file = Pin::new(file);
                        file.poll_complete(cx)
                    }
                    Err(_) => Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("inode `{}` doesn't match a file", self.inode),
                    ))),
                }
            }
            _ => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("inode `{}` doesn't match a file", self.inode),
            ))),
        }
    }
}

impl AsyncWrite for FileHandle {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if !self.writable {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "the file (inode `{}) doesn't have the `write` permission",
                    self.inode
                ),
            )));
        }

        let mut cursor = self.cursor;
        let bytes_written = {
            let mut fs = self.filesystem.inner.write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

            let inode = fs.storage.get_mut(self.inode);
            match inode {
                Some(Node::File(node)) => {
                    let bytes_written = node.file.write(buf, &mut cursor)?;
                    node.metadata.len = node.file.len().try_into().unwrap();
                    bytes_written
                }
                Some(Node::OffloadedFile(node)) => {
                    let bytes_written = node.file.write(OffloadWrite::Buffer(buf), &mut cursor)?;
                    node.metadata.len = node.file.len();
                    bytes_written
                }
                Some(Node::ReadOnlyFile(node)) => {
                    let bytes_written = node.file.write(buf, &mut cursor)?;
                    node.metadata.len = node.file.len().try_into().unwrap();
                    bytes_written
                }
                Some(Node::CustomFile(node)) => {
                    let mut guard = node.file.lock().unwrap();

                    let file = Pin::new(guard.as_mut());
                    if let Err(err) = file.start_seek(io::SeekFrom::Start(self.cursor)) {
                        return Poll::Ready(Err(err));
                    }

                    let file = Pin::new(guard.as_mut());
                    let _ = file.poll_complete(cx);

                    let file = Pin::new(guard.as_mut());
                    let bytes_written = match file.poll_write(cx, buf) {
                        Poll::Ready(Ok(a)) => a,
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                        Poll::Pending => return Poll::Pending,
                    };
                    cursor += bytes_written as u64;
                    node.metadata.len = guard.size();
                    bytes_written
                }
                Some(Node::ArcFile(_)) => {
                    drop(fs);
                    match self.lazy_load_arc_file_mut() {
                        Ok(file) => {
                            let file = Pin::new(file);
                            return file.poll_write(cx, buf);
                        }
                        Err(_) => {
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::NotFound,
                                format!("inode `{}` doesn't match a file", self.inode),
                            )))
                        }
                    }
                }
                _ => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("inode `{}` doesn't match a file", self.inode),
                    )))
                }
            }
        };
        self.cursor = cursor;
        Poll::Ready(Ok(bytes_written))
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let mut cursor = self.cursor;
        let ret = {
            let mut fs = self.filesystem.inner.write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

            let inode = fs.storage.get_mut(self.inode);
            match inode {
                Some(Node::File(node)) => {
                    let buf = bufs
                        .iter()
                        .find(|b| !b.is_empty())
                        .map_or(&[][..], |b| &**b);
                    let bytes_written = node.file.write(buf, &mut cursor)?;
                    node.metadata.len = node.file.buffer.len() as u64;
                    Poll::Ready(Ok(bytes_written))
                }
                Some(Node::OffloadedFile(node)) => {
                    let buf = bufs
                        .iter()
                        .find(|b| !b.is_empty())
                        .map_or(&[][..], |b| &**b);
                    let bytes_written = node.file.write(OffloadWrite::Buffer(buf), &mut cursor)?;
                    node.metadata.len = node.file.len();
                    Poll::Ready(Ok(bytes_written))
                }
                Some(Node::ReadOnlyFile(node)) => {
                    let buf = bufs
                        .iter()
                        .find(|b| !b.is_empty())
                        .map_or(&[][..], |b| &**b);
                    let bytes_written = node.file.write(buf, &mut cursor)?;
                    node.metadata.len = node.file.buffer.len() as u64;
                    Poll::Ready(Ok(bytes_written))
                }
                Some(Node::CustomFile(node)) => {
                    let mut file = node.file.lock().unwrap();
                    let file = Pin::new(file.as_mut());
                    file.poll_write_vectored(cx, bufs)
                }
                Some(Node::ArcFile(_)) => {
                    drop(fs);
                    match self.lazy_load_arc_file_mut() {
                        Ok(file) => {
                            let file = Pin::new(file);
                            file.poll_write_vectored(cx, bufs)
                        }
                        Err(_) => Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::NotFound,
                            format!("inode `{}` doesn't match a file", self.inode),
                        ))),
                    }
                }
                _ => Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("inode `{}` doesn't match a file", self.inode),
                ))),
            }
        };
        self.cursor = cursor;
        ret
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut fs =
            self.filesystem.inner.write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

        let inode = fs.storage.get_mut(self.inode);
        match inode {
            Some(Node::File(node)) => Poll::Ready(node.file.flush()),
            Some(Node::OffloadedFile(node)) => Poll::Ready(node.file.flush()),
            Some(Node::ReadOnlyFile(node)) => Poll::Ready(node.file.flush()),
            Some(Node::CustomFile(node)) => {
                let mut file = node.file.lock().unwrap();
                let file = Pin::new(file.as_mut());
                file.poll_flush(cx)
            }
            Some(Node::ArcFile { .. }) => {
                drop(fs);
                match self.lazy_load_arc_file_mut() {
                    Ok(file) => {
                        let file = Pin::new(file);
                        file.poll_flush(cx)
                    }
                    Err(_) => Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("inode `{}` doesn't match a file", self.inode),
                    ))),
                }
            }
            _ => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("inode `{}` doesn't match a file", self.inode),
            ))),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut fs =
            self.filesystem.inner.write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

        let inode = fs.storage.get_mut(self.inode);
        match inode {
            Some(Node::File { .. }) => Poll::Ready(Ok(())),
            Some(Node::OffloadedFile { .. }) => Poll::Ready(Ok(())),
            Some(Node::ReadOnlyFile { .. }) => Poll::Ready(Ok(())),
            Some(Node::CustomFile(node)) => {
                let mut file = node.file.lock().unwrap();
                let file = Pin::new(file.as_mut());
                file.poll_shutdown(cx)
            }
            Some(Node::ArcFile { .. }) => {
                drop(fs);
                match self.lazy_load_arc_file_mut() {
                    Ok(file) => {
                        let file = Pin::new(file);
                        file.poll_shutdown(cx)
                    }
                    Err(_) => Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("inode `{}` doesn't match a file", self.inode),
                    ))),
                }
            }
            _ => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("inode `{}` doesn't match a file", self.inode),
            ))),
        }
    }

    fn is_write_vectored(&self) -> bool {
        let mut fs = match self.filesystem.inner.write() {
            Ok(a) => a,
            Err(_) => return false,
        };

        let inode = fs.storage.get_mut(self.inode);
        match inode {
            Some(Node::File { .. }) => false,
            Some(Node::OffloadedFile { .. }) => false,
            Some(Node::ReadOnlyFile { .. }) => false,
            Some(Node::CustomFile(node)) => {
                let file = node.file.lock().unwrap();
                file.is_write_vectored()
            }
            Some(Node::ArcFile { .. }) => {
                drop(fs);
                match self.arc_file.as_ref() {
                    Some(Ok(file)) => file.is_write_vectored(),
                    _ => false,
                }
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod test_read_write_seek {
    use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

    use crate::{mem_fs::*, FileSystem as FS};
    use std::io;

    macro_rules! path {
        ($path:expr) => {
            std::path::Path::new($path)
        };
    }

    #[tokio::test]
    async fn test_writing_at_various_positions() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.write(b"foo").await, Ok(3)),
            "writing `foo` at the end of the file",
        );
        assert_eq!(file.size(), 3, "checking the size of the file");

        assert!(
            matches!(file.write(b"bar").await, Ok(3)),
            "writing `bar` at the end of the file",
        );
        assert_eq!(file.size(), 6, "checking the size of the file");

        assert!(
            matches!(file.seek(io::SeekFrom::Start(0)).await, Ok(0)),
            "seeking to 0",
        );

        assert!(
            matches!(file.write(b"baz").await, Ok(3)),
            "writing `baz` at the beginning of the file",
        );
        assert_eq!(file.size(), 6, "checking the size of the file");

        assert!(
            matches!(file.write(b"qux").await, Ok(3)),
            "writing `qux` in the middle of the file",
        );
        assert_eq!(file.size(), 6, "checking the size of the file");

        assert!(
            matches!(file.seek(io::SeekFrom::Start(0)).await, Ok(0)),
            "seeking to 0",
        );

        let mut string = String::new();
        assert!(
            matches!(file.read_to_string(&mut string).await, Ok(6)),
            "reading `bazqux`",
        );
        assert_eq!(string, "bazqux");

        assert!(
            matches!(file.seek(io::SeekFrom::Current(-3)).await, Ok(3)),
            "seeking to 3",
        );

        let mut string = String::new();
        assert!(
            matches!(file.read_to_string(&mut string).await, Ok(3)),
            "reading `qux`",
        );
        assert_eq!(string, "qux");

        assert!(
            matches!(file.seek(io::SeekFrom::End(0)).await, Ok(6)),
            "seeking to 6",
        );

        let mut string = String::new();
        assert!(
            matches!(file.read_to_string(&mut string).await, Ok(0)),
            "reading ``",
        );
        assert_eq!(string, "");
    }

    #[test]
    pub fn writing_to_middle() {
        fn assert_contents(file: &File, expected: &[u8]) {
            let mut buf = vec![0; expected.len() + 1];
            let mut cursor = 0;
            let read = file.read(buf.as_mut(), &mut cursor).unwrap();
            assert_eq!(read, expected.len(), "Must have the same amount of data");
            assert_eq!(buf[0..expected.len()], *expected);
        }

        let mut file = File::new(None);

        let mut cursor = 0;

        // Write to empty file
        file.write(b"hello, world.", &mut cursor).unwrap();
        assert_eq!(cursor, 13);
        assert_contents(&file, b"hello, world.");

        // Write to end of file
        file.write(b"goodbye!", &mut cursor).unwrap();
        assert_eq!(cursor, 21);
        assert_contents(&file, b"hello, world.goodbye!");

        // Write to middle of file
        cursor = 5;
        file.write(b"BOOM", &mut cursor).unwrap();
        assert_eq!(cursor, 9);
        assert_contents(&file, b"helloBOOMrld.goodbye!");

        // Write to middle of file until last byte
        cursor = 17;
        file.write(b"BANG", &mut cursor).unwrap();
        assert_eq!(cursor, 21);
        assert_contents(&file, b"helloBOOMrld.goodBANG");

        // Write to middle past end of file
        cursor = 17;
        file.write(b"OUCH!", &mut cursor).unwrap();
        assert_eq!(cursor, 22);
        assert_contents(&file, b"helloBOOMrld.goodOUCH!");
    }

    #[tokio::test]
    async fn test_reading() {
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
            matches!(file.write(b"foobarbazqux").await, Ok(12)),
            "writing `foobarbazqux`",
        );

        assert!(
            matches!(file.seek(io::SeekFrom::Start(0)).await, Ok(0)),
            "seeking to 0",
        );

        let mut buffer = [0; 6];
        assert!(
            matches!(file.read(&mut buffer[..]).await, Ok(6)),
            "reading 6 bytes",
        );
        assert_eq!(buffer, b"foobar"[..], "checking the 6 bytes");

        assert!(
            matches!(file.seek(io::SeekFrom::Start(0)).await, Ok(0)),
            "seeking to 0",
        );

        let mut buffer = [0; 16];
        assert!(
            matches!(file.read(&mut buffer[..]).await, Ok(12)),
            "reading more bytes than available",
        );
        assert_eq!(buffer[..12], b"foobarbazqux"[..], "checking the 12 bytes");
        assert!(
            matches!(fs.metadata(path!("/foo.txt")), Ok(Metadata { len: 12, .. })),
            "checking the `metadata.len` is 0",
        );
    }

    #[tokio::test]
    async fn test_reading_to_the_end() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.write(b"foobarbazqux").await, Ok(12)),
            "writing `foobarbazqux`",
        );

        assert!(
            matches!(file.seek(io::SeekFrom::Start(0)).await, Ok(0)),
            "seeking to 0",
        );

        let mut buffer = Vec::new();
        assert!(
            matches!(file.read_to_end(&mut buffer).await, Ok(12)),
            "reading all bytes",
        );
        assert_eq!(buffer, b"foobarbazqux"[..], "checking all the bytes");
    }

    #[tokio::test]
    async fn test_reading_to_string() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.write(b"foobarbazqux").await, Ok(12)),
            "writing `foobarbazqux`",
        );

        assert!(
            matches!(file.seek(io::SeekFrom::Start(6)).await, Ok(6)),
            "seeking to 0",
        );

        let mut string = String::new();
        assert!(
            matches!(file.read_to_string(&mut string).await, Ok(6)),
            "reading a string",
        );
        assert_eq!(string, "bazqux", "checking the string");
    }

    #[tokio::test]
    async fn test_reading_exact_buffer() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.write(b"foobarbazqux").await, Ok(12)),
            "writing `foobarbazqux`",
        );

        assert!(
            matches!(file.seek(io::SeekFrom::Start(6)).await, Ok(6)),
            "seeking to 0",
        );

        let mut buffer = [0; 16];
        assert!(
            matches!(file.read_exact(&mut buffer).await, Err(_)),
            "failing to read an exact buffer",
        );

        assert!(
            matches!(file.seek(io::SeekFrom::End(-5)).await, Ok(7)),
            "seeking to 7",
        );

        let mut buffer = [0; 3];
        assert!(
            matches!(file.read_exact(&mut buffer).await, Ok(_)),
            "failing to read an exact buffer",
        );
    }
}

impl fmt::Debug for FileHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileHandle")
            .field("inode", &self.inode)
            .field("readable", &self.readable)
            .field("writable", &self.writable)
            .finish()
    }
}

/// The real file! It is simply a buffer of bytes with a cursor that
/// represents a read/write position in the buffer.
#[derive(Debug)]
pub(super) struct File {
    buffer: TrackedVec,
}

impl File {
    pub(super) fn new(limiter: Option<crate::limiter::DynFsMemoryLimiter>) -> Self {
        Self {
            buffer: TrackedVec::new(limiter),
        }
    }

    pub(super) fn truncate(&mut self) {
        self.buffer.clear();
    }

    pub(super) fn len(&self) -> usize {
        self.buffer.len()
    }
}

impl File {
    pub fn read(&self, buf: &mut [u8], cursor: &mut u64) -> io::Result<usize> {
        let cur_pos = *cursor as usize;
        let max_to_read = cmp::min(self.buffer.len() - cur_pos, buf.len());
        let data_to_copy = &self.buffer[cur_pos..][..max_to_read];

        // SAFETY: `buf[..max_to_read]` and `data_to_copy` have the same size, due to
        // how `max_to_read` is computed.
        buf[..max_to_read].copy_from_slice(data_to_copy);

        *cursor += max_to_read as u64;

        Ok(max_to_read)
    }
}

impl File {
    pub fn seek(&self, position: io::SeekFrom, cursor: &mut u64) -> io::Result<u64> {
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
                TryInto::<i64>::try_into(*cursor).map_err(to_err)? + offset
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
        let next_cursor = next_cursor.try_into().map_err(to_err)?;
        *cursor = cmp::min(self.buffer.len() as u64, next_cursor);

        let cursor = *cursor;
        Ok(cursor)
    }
}

impl File {
    pub fn write(&mut self, buf: &[u8], cursor: &mut u64) -> io::Result<usize> {
        let position = *cursor as usize;

        if position + buf.len() > self.buffer.len() {
            // Writing past the end of the current buffer, must reallocate
            let len_after_end = (position + buf.len()) - self.buffer.len();
            let let_to_end = buf.len() - len_after_end;
            self.buffer[position..position + let_to_end].copy_from_slice(&buf[0..let_to_end]);
            self.buffer.extend_from_slice(&buf[let_to_end..buf.len()])?;
        } else {
            self.buffer[position..position + buf.len()].copy_from_slice(buf);
        }

        *cursor += buf.len() as u64;

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Read only file that uses copy-on-write
#[derive(Debug)]
pub(super) struct ReadOnlyFile {
    buffer: Cow<'static, [u8]>,
}

impl ReadOnlyFile {
    pub(super) fn new(buffer: Cow<'static, [u8]>) -> Self {
        Self { buffer }
    }

    pub(super) fn len(&self) -> usize {
        self.buffer.len()
    }
}

impl ReadOnlyFile {
    pub fn read(&self, buf: &mut [u8], cursor: &mut u64) -> io::Result<usize> {
        let cur_pos = *cursor as usize;
        let max_to_read = cmp::min(self.buffer.len() - cur_pos, buf.len());
        let data_to_copy = &self.buffer[cur_pos..][..max_to_read];

        // SAFETY: `buf[..max_to_read]` and `data_to_copy` have the same size, due to
        // how `max_to_read` is computed.
        buf[..max_to_read].copy_from_slice(data_to_copy);

        *cursor += max_to_read as u64;

        Ok(max_to_read)
    }
}

impl ReadOnlyFile {
    pub fn seek(&self, _position: io::SeekFrom, _cursor: &mut u64) -> io::Result<u64> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "file is read-only",
        ))
    }
}

impl ReadOnlyFile {
    pub fn write(&mut self, _buf: &[u8], _cursor: &mut u64) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "file is read-only",
        ))
    }

    pub fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
