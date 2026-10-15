use std::sync::{Arc, Mutex};

use wasixv2_abi::OpenFlags;
use wasixv2_syscalls::{Errno, Fd, LocalFuture};
use wasixv2_vfs::Inode;

/// A stream an open file can refer to instead of an inode: a pipe, a socket, a host stream.
pub trait FileObject: Send + Sync {
    fn read<'a>(&'a self, buf: &'a mut [u8]) -> LocalFuture<'a, Result<usize, Errno>>;
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, Result<usize, Errno>>;
}

pub(crate) enum Backing {
    Stream(Arc<dyn FileObject>),
    Inode(Arc<dyn Inode>),
}

/// Flags `F_SETFL` can change.
const SETFL_MASK: OpenFlags = OpenFlags(OpenFlags::O_APPEND.0 | OpenFlags::O_NONBLOCK.0);

/// An open file description: the part `dup` and `fork` share.
pub(crate) struct OpenFile {
    pub(crate) backing: Backing,
    /// The access mode and status flags, as `F_GETFL` reports them.
    flags: Mutex<OpenFlags>,
    /// The file position; unused for streams.
    pos: Mutex<u64>,
}

impl OpenFile {
    pub(crate) fn new(backing: Backing, flags: OpenFlags) -> Self {
        let kept = OpenFlags::O_ACCMODE | SETFL_MASK;
        Self {
            backing,
            flags: Mutex::new(flags & kept),
            pos: Mutex::new(0),
        }
    }

    pub(crate) fn flags(&self) -> OpenFlags {
        *self.flags.lock().unwrap()
    }

    pub(crate) fn set_status_flags(&self, flags: OpenFlags) {
        let mut current = self.flags.lock().unwrap();
        *current = (*current & !SETFL_MASK) | (flags & SETFL_MASK);
    }

    fn access(&self) -> OpenFlags {
        self.flags() & OpenFlags::O_ACCMODE
    }

    /// `EBADF` unless the file was opened for reading, as Linux checks `FMODE_READ`.
    pub(crate) fn check_readable(&self) -> Result<(), Errno> {
        match self.access() {
            OpenFlags::O_RDONLY | OpenFlags::O_RDWR => Ok(()),
            _ => Err(Errno::EBADF),
        }
    }

    pub(crate) fn check_writable(&self) -> Result<(), Errno> {
        match self.access() {
            OpenFlags::O_WRONLY | OpenFlags::O_RDWR => Ok(()),
            _ => Err(Errno::EBADF),
        }
    }

    /// `ESPIPE` for streams, which have no file position.
    pub(crate) fn check_seekable(&self) -> Result<(), Errno> {
        self.inode().map(drop)
    }

    fn inode(&self) -> Result<&Arc<dyn Inode>, Errno> {
        match &self.backing {
            Backing::Inode(inode) => Ok(inode),
            Backing::Stream(_) => Err(Errno::ESPIPE),
        }
    }

    pub(crate) async fn read(&self, buf: &mut [u8]) -> Result<usize, Errno> {
        let inode = match &self.backing {
            Backing::Stream(object) => return object.read(buf).await,
            Backing::Inode(inode) => inode,
        };
        let pos = *self.pos.lock().unwrap();
        let n = inode.read_at(buf, pos).await?;
        *self.pos.lock().unwrap() = pos + n as u64;
        Ok(n)
    }

    pub(crate) async fn write(&self, data: &[u8]) -> Result<usize, Errno> {
        let inode = match &self.backing {
            Backing::Stream(object) => return object.write(data).await,
            Backing::Inode(inode) => inode,
        };
        let pos = if self.flags().contains(OpenFlags::O_APPEND) {
            inode.size()
        } else {
            *self.pos.lock().unwrap()
        };
        let n = inode.write_at(data, pos).await?;
        *self.pos.lock().unwrap() = pos + n as u64;
        Ok(n)
    }

    /// Reads at `offset` without moving the file position, as `pread` does.
    pub(crate) async fn read_at(&self, buf: &mut [u8], offset: u64) -> Result<usize, Errno> {
        self.inode()?.read_at(buf, offset).await
    }

    pub(crate) async fn write_at(&self, data: &[u8], offset: u64) -> Result<usize, Errno> {
        self.inode()?.write_at(data, offset).await
    }

    /// Moves the file position, as `lseek` does, and returns the new one.
    pub(crate) fn seek(&self, base: SeekBase, offset: i64) -> Result<u64, Errno> {
        let inode = self.inode()?;
        let mut pos = self.pos.lock().unwrap();
        let base = match base {
            SeekBase::Start => 0,
            SeekBase::Current => *pos,
            SeekBase::End => inode.size(),
        };
        let new = i64::try_from(base)
            .ok()
            .and_then(|base| base.checked_add(offset))
            .and_then(|new| u64::try_from(new).ok())
            .ok_or(Errno::EINVAL)?;
        *pos = new;
        Ok(new)
    }
}

pub(crate) enum SeekBase {
    Start,
    Current,
    End,
}

struct Slot {
    file: Arc<OpenFile>,
    cloexec: bool,
}

#[derive(Default)]
pub(crate) struct FdTable {
    slots: Vec<Option<Slot>>,
}

impl FdTable {
    pub(crate) fn get(&self, fd: Fd) -> Option<Arc<OpenFile>> {
        Some(self.slot(fd)?.file.clone())
    }

    /// Installs `file` at the lowest free descriptor, as `open` does.
    pub(crate) fn install(&mut self, file: Arc<OpenFile>, cloexec: bool) -> Fd {
        let index = match self.slots.iter().position(Option::is_none) {
            Some(index) => index,
            None => {
                self.slots.push(None);
                self.slots.len() - 1
            }
        };
        self.slots[index] = Some(Slot { file, cloexec });
        Fd(index as i32)
    }

    pub(crate) fn remove(&mut self, fd: Fd) -> Option<Arc<OpenFile>> {
        let index = usize::try_from(fd.0).ok()?;
        Some(self.slots.get_mut(index)?.take()?.file)
    }

    pub(crate) fn cloexec(&self, fd: Fd) -> Option<bool> {
        Some(self.slot(fd)?.cloexec)
    }

    pub(crate) fn set_cloexec(&mut self, fd: Fd, cloexec: bool) -> Option<()> {
        let index = usize::try_from(fd.0).ok()?;
        self.slots.get_mut(index)?.as_mut()?.cloexec = cloexec;
        Some(())
    }

    fn slot(&self, fd: Fd) -> Option<&Slot> {
        let index = usize::try_from(fd.0).ok()?;
        self.slots.get(index)?.as_ref()
    }
}
