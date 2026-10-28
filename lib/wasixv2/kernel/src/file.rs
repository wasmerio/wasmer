use std::{
    any::Any,
    cell::Cell,
    collections::BTreeSet,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU32, Ordering},
    },
};

use wasixv2_abi::{IoctlRequest, OpenFlags};
use wasixv2_syscalls::{Caller, Errno, Fd, LocalFuture, SysResult};
use wasixv2_vfs::{File, FileType, LockKey, Path, SeekHole, Vfs};
use wasixv2_wait::{Interrupt, NoInterrupt, PollMask, PollTable, Pollable};

use crate::epoll::EpollLink;

/// What an object needs to know about the call it serves: the open file's status flags as the
/// call began (Linux reads `f_flags` once) and how to notice a signal while blocked.
#[derive(Clone, Copy)]
pub struct Io<'a> {
    pub flags: OpenFlags,
    pub interrupt: &'a dyn Interrupt,
    /// Where a write reports that it found the reading side gone, whatever it returns: the
    /// caller then raises `SIGPIPE` even for a partial count, as `pipe_write` does. `None`
    /// where nobody raises it (reads, and writes the caller signals by their `EPIPE`).
    pub broken: Option<&'a Cell<bool>>,
}

impl Io<'_> {
    /// `O_NONBLOCK`: fail with `EAGAIN` instead of waiting.
    pub fn nonblocking(&self) -> bool {
        self.flags.contains(OpenFlags::O_NONBLOCK)
    }

    /// Reports that the reading side is gone (`!pipe->readers`); see [`Io::broken`].
    pub fn set_broken(&self) {
        if let Some(broken) = self.broken {
            broken.set(true);
        }
    }
}

/// Where a read's data goes, as Linux's `iov_iter`: the caller's buffers in guest memory, which
/// a fault may cut short.
pub trait ReadSink {
    /// Copies `data` to the next bytes and returns how many arrived; fewer than `data.len()`
    /// means a fault ended the copy.
    fn copy_out<'a>(&'a mut self, data: &'a [u8]) -> LocalFuture<'a, usize>;
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

    /// `read_iter`: reads at most `buf.len()` bytes, with `buf` as the kernel's buffer, copies
    /// them to `to` and returns how many arrived there; `EFAULT` if there was data and none
    /// arrived. This default reads with `read` and copies what it got, so a fault loses what
    /// did not arrive, as Linux's objects that take their data before copying it out do
    /// (eventfd, timerfd, signalfd, inotify, datagram sockets). Streams override it to take
    /// nothing a fault kept from the user ([`taken`]).
    fn read_to<'a>(
        &'a self,
        io: Io<'a>,
        buf: &'a mut [u8],
        to: &'a mut dyn ReadSink,
    ) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            let n = self.read(io, buf).await?;
            delivered(n, to.copy_out(&buf[..n]).await)
        })
    }

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

    /// Whether the object has a position, as a device does (`FMODE_PREAD`, `FMODE_PWRITE`,
    /// and an `llseek`): `pread` and `pwrite` then go to `read` and `write`, and `lseek` to
    /// [`FileObject::llseek`]. Streams have none and fail those with `ESPIPE`.
    fn positional(&self) -> bool {
        false
    }

    /// Whether each write is one message (datagram and seqpacket sockets): it reaches the
    /// object whole, never split into steps, and `EPIPE` from it raises no `SIGPIPE`, which
    /// only byte streams do (`pipe_write`, `sk_stream_error`, `unix_stream_sendmsg`).
    fn messages(&self) -> bool {
        false
    }

    /// Whether a read fills the whole buffer unless it fails, as the memory and random devices
    /// do (`read_iter_zero`, `urandom_read_iter`). A read larger than one kernel step then goes
    /// on in steps, ending early only at a signal. Other objects give what one read of them
    /// has, which may be less than asked: a pipe's or socket's data so far, one message, one
    /// event.
    fn fills_reads(&self) -> bool {
        false
    }

    /// Whether `sendfile` can read from the object (`f_op->splice_read`): sockets can; pipes
    /// and event objects cannot.
    fn splice_read(&self) -> bool {
        false
    }

    /// Whether `splice` can write to the object from a pipe (`f_op->splice_write`): sockets
    /// and the memory and random devices can; event objects cannot.
    fn splice_write(&self) -> bool {
        false
    }

    /// Bytes a read would find now (what `FIONREAD` reports): a stream's buffered data, a
    /// datagram socket's next message. Reads of other objects give at most one kernel step;
    /// a read of this one sizes its buffer to these bytes instead, so it takes all a pipe or
    /// stream socket holds (`pipe_read`, `tcp_recvmsg`) and a datagram whole.
    fn buffered(&self) -> usize {
        0
    }

    /// `llseek` of a positional object, from the position `pos`; returns the new position.
    fn llseek(&self, pos: u64, base: SeekBase, offset: i64) -> Result<u64, Errno> {
        let _ = (pos, base, offset);
        Err(Errno::ESPIPE)
    }
}

/// What a read that had `n` bytes returns once `copied` of them arrived: the count, or `EFAULT`
/// when none did (`copy_page_to_iter` short in `pipe_read`, `-EFAULT` from a recv actor).
pub(crate) fn delivered(n: usize, copied: usize) -> SysResult<usize> {
    match copied == 0 && n > 0 {
        true => Err(Errno::EFAULT.into()),
        false => Ok(copied),
    }
}

/// What a stream read that copied `copied` of the `n` bytes it looked at returns, and takes:
/// all of them, or on a fault none with `EFAULT`. Linux's streams take data a buffer or an
/// skb at a time and give up a whole one only once all of it was copied (`pipe_read` checks
/// `copy_page_to_iter` for a short copy, `unix_stream_read_actor` and `tcp_recvmsg` for a
/// failed `skb_copy_datagram_msg`); a read here takes one step, so the step is that unit.
pub(crate) fn taken(n: usize, copied: usize) -> SysResult<usize> {
    match copied == n {
        true => Ok(n),
        false => Err(Errno::EFAULT.into()),
    }
}

pub(crate) enum Backing {
    /// An object that is not an inode's data, and the path of the inode it belongs to: its
    /// pipefs, sockfs or anonymous inode, or the FIFO or device node it was opened through.
    Stream(Arc<dyn FileObject>, Path),
    /// A filesystem object, opened through the VFS.
    Inode(File),
}

/// Flags `F_SETFL` can change (`SETFL_MASK`).
const SETFL_MASK: OpenFlags = OpenFlags(
    OpenFlags::O_APPEND.0
        | OpenFlags::O_NONBLOCK.0
        | OpenFlags::O_NOATIME.0
        | OpenFlags::O_DIRECT.0,
);
/// Open flags an open file does not keep (`do_dentry_open`, and `build_open_flags` for
/// `O_CLOEXEC`, which belongs to the descriptor): `F_GETFL` reports all the others.
const OPEN_ONLY_FLAGS: OpenFlags = OpenFlags(
    OpenFlags::O_CREAT.0
        | OpenFlags::O_EXCL.0
        | OpenFlags::O_NOCTTY.0
        | OpenFlags::O_TRUNC.0
        | OpenFlags::O_CLOEXEC.0,
);

/// An open file description: the part `dup` and `fork` share.
pub(crate) struct OpenFile {
    pub(crate) backing: Backing,
    /// The access mode and status flags, as `F_GETFL` reports them.
    flags: AtomicU32,
    /// The file position; unused for streams.
    pos: Mutex<u64>,
    /// Held across a call that reads and then moves the position, so threads sharing the
    /// description never use one position twice (`f_pos_lock`, `FMODE_ATOMIC_POS`).
    pos_lock: futures_util::lock::Mutex<()>,
    /// The epoll items watching this file, removed when it goes away.
    pub(crate) epoll_links: Mutex<Vec<EpollLink>>,
    /// A stream's `flock` identity, made by its first `flock`.
    stream_flock: OnceLock<StreamFlock>,
}

/// What a stream (pipe, socket) is `flock`ed as: its pseudo inode, and an owner of its own as
/// a VFS file has. Dropped with the open file, which releases the lock (`locks_remove_file`).
pub(crate) struct StreamFlock {
    vfs: Arc<Vfs>,
    pub(crate) key: LockKey,
    pub(crate) id: u64,
}

impl Drop for StreamFlock {
    fn drop(&mut self) {
        self.vfs.locks.release_file(self.key, self.id);
    }
}

impl OpenFile {
    pub(crate) fn new(backing: Backing, flags: OpenFlags) -> Self {
        Self {
            backing,
            flags: AtomicU32::new((flags & !OPEN_ONLY_FLAGS).0),
            pos: Mutex::new(0),
            pos_lock: futures_util::lock::Mutex::new(()),
            epoll_links: Mutex::default(),
            stream_flock: OnceLock::new(),
        }
    }

    /// The lock key and owner `flock` uses for this file: a VFS file's own, or for a stream
    /// its inode's and one made now.
    pub(crate) fn flock_owner(&self, vfs: &Arc<Vfs>) -> (LockKey, u64) {
        match &self.backing {
            Backing::Inode(file) => (file.lock_key(), file.id),
            Backing::Stream(_, path) => {
                let lock = self.stream_flock.get_or_init(|| StreamFlock {
                    vfs: vfs.clone(),
                    key: (path.dentry.sb().id, path.dentry.node().ino()),
                    id: vfs.next_id(),
                });
                (lock.key, lock.id)
            }
        }
    }

    /// Read on every I/O call, so an atomic rather than a lock; `F_SETFL` is the only writer.
    pub(crate) fn flags(&self) -> OpenFlags {
        OpenFlags(self.flags.load(Ordering::Relaxed))
    }

    /// `F_SETFL`'s `O_DIRECT` check: only a pipe (packet mode) or a file whose data the
    /// filesystem can move directly (`FMODE_CAN_ODIRECT`: regular files here) takes it.
    pub(crate) fn can_direct(&self) -> bool {
        match &self.backing {
            Backing::Inode(file) => file.file_type() == FileType::Regular,
            Backing::Stream(_, path) => path.dentry.file_type() == FileType::Fifo,
        }
    }

    pub(crate) fn set_status_flags(&self, flags: OpenFlags) {
        let keep = |current: u32| (current & !SETFL_MASK.0) | (flags & SETFL_MASK).0;
        let _ = self
            .flags
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |c| Some(keep(c)));
    }

    /// Serialises position updates for the duration of a call (`fdget_pos`).
    pub(crate) async fn lock_pos(&self) -> futures_util::lock::MutexGuard<'_, ()> {
        self.pos_lock.lock().await
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
        if self.flags().contains(OpenFlags::O_PATH) {
            return Err(Errno::EBADF);
        }
        match self.access() {
            OpenFlags::O_RDONLY | OpenFlags::O_RDWR => Ok(()),
            _ => Err(Errno::EBADF),
        }
    }

    pub(crate) fn check_writable(&self) -> Result<(), Errno> {
        if self.flags().contains(OpenFlags::O_PATH) {
            return Err(Errno::EBADF);
        }
        match self.access() {
            OpenFlags::O_WRONLY | OpenFlags::O_RDWR => Ok(()),
            _ => Err(Errno::EBADF),
        }
    }

    /// `ESPIPE` for streams, which have no file position (`FMODE_PREAD` and `FMODE_PWRITE`).
    pub(crate) fn check_seekable(&self) -> Result<(), Errno> {
        match self.positional() {
            Some(_) => Ok(()),
            None => self.inode().map(drop),
        }
    }

    /// The VFS file behind this description; `ESPIPE` for streams.
    pub(crate) fn inode(&self) -> Result<&File, Errno> {
        match &self.backing {
            Backing::Inode(inode) => Ok(inode),
            Backing::Stream(..) => Err(Errno::ESPIPE),
        }
    }

    /// The object behind a non-inode file.
    pub(crate) fn object(&self) -> Option<&Arc<dyn FileObject>> {
        match &self.backing {
            Backing::Stream(object, _) => Some(object),
            Backing::Inode(_) => None,
        }
    }

    /// Where the file is (`f_path`): its VFS path, or the inode of its object.
    pub(crate) fn path(&self) -> &Path {
        match &self.backing {
            Backing::Stream(_, path) => path,
            Backing::Inode(file) => &file.path,
        }
    }

    /// The object behind the file, if it is a `T`.
    pub(crate) fn downcast<T: FileObject>(&self) -> Option<&T> {
        let object: &dyn Any = &**self.object()?;
        object.downcast_ref()
    }

    /// Whether a read goes on in steps until the buffer is full: a file's does, and an object's
    /// that [`FileObject::fills_reads`].
    pub(crate) fn fills_reads(&self) -> bool {
        self.object().is_none_or(|object| object.fills_reads())
    }

    /// What a read would find now; see `FileObject::buffered`.
    pub(crate) fn buffered(&self) -> usize {
        self.object().map_or(0, |object| object.buffered())
    }

    /// Whether writes to this file are messages; see `FileObject::messages`.
    pub(crate) fn messages(&self) -> bool {
        self.object().is_some_and(|object| object.messages())
    }

    /// `file_can_poll`: regular files and directories cannot be watched by epoll.
    pub(crate) fn can_poll(&self) -> bool {
        self.object().is_some_and(|object| object.can_poll())
    }

    /// `read` at the file position. A blocked read ends early with `SysErr::Restart` when
    /// `interrupt` becomes pending, as every blocking read does on Linux, vectored or not.
    pub(crate) async fn read(&self, interrupt: &dyn Interrupt, buf: &mut [u8]) -> SysResult<usize> {
        let file = match &self.backing {
            Backing::Stream(object, _) => return object.read(self.io(interrupt), buf).await,
            Backing::Inode(file) => file,
        };
        let _pos = self.lock_pos().await;
        let pos = *self.pos.lock().unwrap();
        let n = file.read_at(buf, pos).await?;
        *self.pos.lock().unwrap() = pos + n as u64;
        Ok(n)
    }

    /// What an object's call made now needs to know: the status flags and how to notice a
    /// signal.
    pub(crate) fn io<'a>(&self, interrupt: &'a dyn Interrupt) -> Io<'a> {
        Io {
            flags: self.flags(),
            interrupt,
            broken: None,
        }
    }

    /// `write` at the file position (or the end, for `O_APPEND`). A blocked write ends early
    /// with `SysErr::Restart`, or the bytes moved so far, when `interrupt` becomes pending.
    /// `broken` learns whether a stream found its reading side gone ([`Io::broken`]).
    pub(crate) async fn write(
        &self,
        interrupt: &dyn Interrupt,
        broken: &Cell<bool>,
        data: &[u8],
    ) -> SysResult<usize> {
        let io = Io {
            flags: self.flags(),
            interrupt,
            broken: Some(broken),
        };
        let file = match &self.backing {
            Backing::Stream(object, _) => return object.write(io, data).await,
            Backing::Inode(file) => file,
        };
        let _pos = self.lock_pos().await;
        if self.flags().contains(OpenFlags::O_APPEND) {
            let (end, n) = file.append(data).await?;
            *self.pos.lock().unwrap() = end;
            return Ok(n);
        }
        let pos = *self.pos.lock().unwrap();
        check_file_size(pos, data.len())?;
        let n = file.write_at(data, pos).await?;
        *self.pos.lock().unwrap() = pos + n as u64;
        Ok(n)
    }

    /// Reads at `offset` without moving the file position, as `pread` does.
    pub(crate) async fn read_at(&self, buf: &mut [u8], offset: u64) -> SysResult<usize> {
        if let Some(object) = self.positional() {
            return object.read(self.io(&NoInterrupt), buf).await;
        }
        Ok(self.inode()?.read_at(buf, offset).await?)
    }

    /// The object of a positional non-inode file (a device).
    fn positional(&self) -> Option<&Arc<dyn FileObject>> {
        self.object().filter(|object| object.positional())
    }

    pub(crate) fn pos(&self) -> u64 {
        *self.pos.lock().unwrap()
    }

    pub(crate) fn set_pos(&self, pos: u64) {
        *self.pos.lock().unwrap() = pos;
    }

    /// Moves the file position, as `lseek` does, and returns the new one.
    pub(crate) async fn seek(&self, base: SeekBase, offset: i64) -> Result<u64, Errno> {
        if let Some(object) = self.positional() {
            let _pos = self.lock_pos().await;
            let new = object.llseek(self.pos(), base, offset)?;
            self.set_pos(new);
            return Ok(new);
        }
        let file = self.inode()?;
        if file.path_only {
            return Err(Errno::EBADF);
        }
        let _pos = self.lock_pos().await;
        let current = self.pos();
        let is_dir = file.file_type() == FileType::Directory;
        let new = match base {
            SeekBase::Start | SeekBase::Current => {
                let base = if base == SeekBase::Start { 0 } else { current };
                checked_offset(base, offset)?
            }
            // tmpfs directories only seek from the start or the current position.
            _ if is_dir => return Err(Errno::EINVAL),
            SeekBase::End => {
                let size = file.node().getattr().await?.size;
                checked_offset(size, offset)?
            }
            SeekBase::Data | SeekBase::Hole => {
                let offset = u64::try_from(offset).map_err(|_| Errno::ENXIO)?;
                let which = if base == SeekBase::Data {
                    SeekHole::Data
                } else {
                    SeekHole::Hole
                };
                file.seek_hole(offset, which).await?
            }
        };
        self.set_pos(new);
        Ok(new)
    }
}

impl Drop for OpenFile {
    /// `eventpoll_release`: the last reference is gone, so no epoll watches the file any more.
    fn drop(&mut self) {
        let links = std::mem::take(self.epoll_links.get_mut().unwrap());
        crate::epoll::release(self, links);
    }
}

impl Pollable for OpenFile {
    /// `vfs_poll`. Inodes have no readiness of their own and are always ready, as Linux's
    /// `DEFAULT_POLLMASK` for files without a poll operation.
    fn poll(&self, table: Option<&mut PollTable<'_>>) -> PollMask {
        match &self.backing {
            Backing::Stream(object, _) => object.poll(table),
            Backing::Inode(_) => PollMask::DEFAULT,
        }
    }
}

/// `base + offset`, which must land in `0..=i64::MAX` (`vfs_setpos`).
fn checked_offset(base: u64, offset: i64) -> Result<u64, Errno> {
    i64::try_from(base)
        .ok()
        .and_then(|base| base.checked_add(offset))
        .and_then(|new| u64::try_from(new).ok())
        .ok_or(Errno::EINVAL)
}

/// Writes may not reach past the largest file offset (`generic_write_check_limits`).
pub(crate) fn check_file_size(pos: u64, len: usize) -> Result<(), Errno> {
    if len > 0 && pos >= i64::MAX as u64 {
        return Err(Errno::EFBIG);
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SeekBase {
    Start,
    Current,
    End,
    Data,
    Hole,
}

struct Slot {
    file: Arc<OpenFile>,
    cloexec: bool,
}

#[derive(Default)]
pub(crate) struct FdTable {
    slots: Vec<Option<Slot>>,
    /// Descriptors handed out for a file still being opened (`get_unused_fd_flags` before
    /// `fd_install`): no other allocation takes them, and they are not open yet.
    reserved: BTreeSet<usize>,
    /// The soft `RLIMIT_NOFILE` once a process set one; no descriptor at or above it is handed
    /// out.
    limit: Option<usize>,
}

impl FdTable {
    pub(crate) fn set_limit(&mut self, limit: usize) {
        self.limit = Some(limit);
    }

    /// The soft `RLIMIT_NOFILE` in force.
    pub(crate) fn limit(&self) -> usize {
        self.limit.unwrap_or(crate::fs::NOFILE)
    }

    pub(crate) fn get(&self, fd: Fd) -> Option<Arc<OpenFile>> {
        Some(self.slot(fd)?.file.clone())
    }

    /// Installs `file` at the lowest free descriptor, as `open` does; `None` if that would reach
    /// the limit (`EMFILE`).
    pub(crate) fn install(&mut self, file: Arc<OpenFile>, cloexec: bool) -> Option<Fd> {
        self.install_from(0, file, cloexec)
    }

    /// Installs `file` at the lowest free descriptor at or above `min`, as `F_DUPFD` does;
    /// `None` if that would reach the limit.
    pub(crate) fn install_from(
        &mut self,
        min: usize,
        file: Arc<OpenFile>,
        cloexec: bool,
    ) -> Option<Fd> {
        let index = self.free_from(min).next()?;
        if self.slots.len() <= index {
            self.slots.resize_with(index + 1, || None);
        }
        self.slots[index] = Some(Slot { file, cloexec });
        Some(Fd(index as i32))
    }

    /// Puts `file` at `fd`, as `dup2` does, and returns what was there.
    pub(crate) fn install_at(
        &mut self,
        fd: Fd,
        file: Arc<OpenFile>,
        cloexec: bool,
    ) -> Option<Arc<OpenFile>> {
        let index = fd.0 as usize;
        if self.slots.len() <= index {
            self.slots.resize_with(index + 1, || None);
        }
        self.slots[index]
            .replace(Slot { file, cloexec })
            .map(|slot| slot.file)
    }

    /// Whether `n` descriptors are free below the limit, for calls that install several at
    /// once and must not install some of them only.
    pub(crate) fn has_room(&self, n: usize) -> bool {
        self.free_from(0).take(n).count() == n
    }

    fn free_from(&self, min: usize) -> impl Iterator<Item = usize> + '_ {
        (min..self.limit()).filter(|&i| {
            self.slots.get(i).is_none_or(Option::is_none) && !self.reserved.contains(&i)
        })
    }

    /// Reserves the lowest free descriptor; `None` at the limit (`EMFILE`).
    pub(crate) fn reserve(&mut self) -> Option<Fd> {
        let index = self.free_from(0).next()?;
        self.reserved.insert(index);
        Some(Fd(index as i32))
    }

    /// Installs `file` at the reserved `fd` (`fd_install`).
    pub(crate) fn fill(&mut self, fd: Fd, file: Arc<OpenFile>, cloexec: bool) {
        let removed = self.reserved.remove(&(fd.0 as usize));
        debug_assert!(removed, "only reserved descriptors are filled");
        let replaced = self.install_at(fd, file, cloexec);
        debug_assert!(replaced.is_none());
    }

    /// Gives back a reservation that was not filled (`put_unused_fd`).
    pub(crate) fn release(&mut self, fd: Fd) {
        self.reserved.remove(&(fd.0 as usize));
    }

    /// Whether `fd` is reserved for a file still being opened.
    pub(crate) fn is_reserved(&self, fd: Fd) -> bool {
        usize::try_from(fd.0).is_ok_and(|index| self.reserved.contains(&index))
    }

    /// One past the highest descriptor that may be in use.
    pub(crate) fn end(&self) -> usize {
        self.slots.len()
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

    /// Every open descriptor, lowest first, with its close-on-exec flag.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (Fd, &Arc<OpenFile>, bool)> {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            let slot = slot.as_ref()?;
            Some((Fd(index as i32), &slot.file, slot.cloexec))
        })
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
