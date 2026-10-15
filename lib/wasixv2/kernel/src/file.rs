use std::{
    any::Any,
    sync::{Arc, Mutex},
};

use wasixv2_abi::{IoctlRequest, OpenFlags};
use wasixv2_syscalls::{Caller, Errno, Fd, LocalFuture, SysResult};
use wasixv2_vfs::Inode;
use wasixv2_wait::{Interrupt, NoInterrupt, PollMask, PollTable, Pollable};

/// What an object needs to know about the call it serves: the open file's status flags as the
/// call began (Linux reads `f_flags` once) and how to notice a signal while blocked.
#[derive(Clone, Copy)]
pub struct Io<'a> {
    pub flags: OpenFlags,
    pub interrupt: &'a dyn Interrupt,
}

impl Io<'_> {
    /// `O_NONBLOCK`: fail with `EAGAIN` instead of waiting.
    pub fn nonblocking(&self) -> bool {
        self.flags.contains(OpenFlags::O_NONBLOCK)
    }
}

/// The object behind an open file that is not an inode: a pipe, a socket, an event source, a
/// host stream. Linux's `file_operations`: one trait for read, write, poll and ioctl.
///
/// `read` and `write` block (asynchronously) unless `io.nonblocking()`, then fail with
/// `EAGAIN`. A blocked call ends with `SysErr::Restart` when `io.interrupt` becomes pending,
/// or with the bytes moved so far. `Pollable::poll` must agree with what `read` and `write`
/// would do.
///
/// The object is dropped when the last open file referring to it goes away (Linux's
/// `release`), so `Drop` is where a pipe end or a socket closes.
pub trait FileObject: Pollable + Any {
    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>>;
    fn write<'a>(&'a self, io: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>>;

    /// Requests the generic ioctl code did not handle. Linux answers unknown requests with
    /// `ENOTTY`.
    fn ioctl<'a>(
        &'a self,
        caller: &'a Caller<'a>,
        request: IoctlRequest,
        arg: u32,
    ) -> LocalFuture<'a, SysResult<i32>> {
        let _ = (caller, request, arg);
        Box::pin(async { Err(Errno::ENOTTY.into()) })
    }

    /// Linux's `file_can_poll`: whether epoll may watch the object.
    fn can_poll(&self) -> bool {
        true
    }
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

    /// The file position.
    pub(crate) fn position(&self) -> u64 {
        *self.pos.lock().unwrap()
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

    /// The call context for this file's current flags.
    // TODO: carry the calling task's signal state once signals are delivered; until then
    // nothing interrupts a blocked call.
    pub(crate) fn io(&self) -> Io<'static> {
        Io {
            flags: self.flags(),
            interrupt: &NoInterrupt,
        }
    }

    /// The object behind a non-inode file.
    pub(crate) fn object(&self) -> Option<&Arc<dyn FileObject>> {
        match &self.backing {
            Backing::Stream(object) => Some(object),
            Backing::Inode(_) => None,
        }
    }

    /// The object behind the file, if it is a `T`.
    pub(crate) fn downcast<T: FileObject>(&self) -> Option<&T> {
        let object: &dyn Any = &**self.object()?;
        object.downcast_ref()
    }

    /// `file_can_poll`: regular files and directories cannot be watched by epoll.
    pub(crate) fn can_poll(&self) -> bool {
        self.object().is_some_and(|object| object.can_poll())
    }

    pub(crate) async fn read(&self, buf: &mut [u8]) -> SysResult<usize> {
        let inode = match &self.backing {
            Backing::Stream(object) => return object.read(self.io(), buf).await,
            Backing::Inode(inode) => inode,
        };
        let pos = *self.pos.lock().unwrap();
        let n = inode.read_at(buf, pos).await?;
        *self.pos.lock().unwrap() = pos + n as u64;
        Ok(n)
    }

    pub(crate) async fn write(&self, data: &[u8]) -> SysResult<usize> {
        let inode = match &self.backing {
            Backing::Stream(object) => return object.write(self.io(), data).await,
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

impl Pollable for OpenFile {
    /// `vfs_poll`. Inodes have no readiness of their own and are always ready, as Linux's
    /// `DEFAULT_POLLMASK` for files without a poll operation.
    fn poll(&self, table: Option<&mut PollTable<'_>>) -> PollMask {
        match &self.backing {
            Backing::Stream(object) => object.poll(table),
            Backing::Inode(_) => PollMask::DEFAULT,
        }
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

    /// The table's capacity as Linux's `fdtable.max_fds` would be: 32 descriptors, then grown
    /// in power-of-two multiples of 256 (`alloc_fdtable` on a 32-bit kernel). `select` ignores
    /// descriptors at or above it.
    pub(crate) fn max_fds(&self) -> usize {
        const DEFAULT: usize = 32;
        const STEP: usize = 1024 / 4;
        match self.slots.len() {
            len if len <= DEFAULT => DEFAULT,
            len => ((len - 1) / STEP + 1).next_power_of_two() * STEP,
        }
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
