//! Character devices: the drivers behind device nodes, found by device number when a node is
//! opened (`chrdev_open`). The memory devices follow `drivers/char/mem.c`, the random devices
//! `drivers/char/random.c`; `/dev/tty` is `tty_open` without a controlling terminal.
//!
//! The nodes themselves live on an ordinary filesystem, as on Linux, where devtmpfs is a tmpfs:
//! [`Kernel::create_device_nodes`] makes the standard ones.

use std::sync::Arc;

use wasixv2_host::Host;
use wasixv2_syscalls::{Errno, LocalFuture, SysResult};
use wasixv2_vfs::{Cred, Ctx, Device, File, FileType, FsResult, Vfs};
use wasixv2_wait::{PollMask, PollTable, Pollable};

use crate::{
    Kernel,
    file::{FileObject, Io, SeekBase},
};

/// `MEM_MAJOR`, with `/dev/null`, `/dev/zero`, `/dev/full`, `/dev/random` and `/dev/urandom`.
const MEM_MAJOR: u32 = 1;
/// `TTYAUX_MAJOR`, with `/dev/tty` (and `/dev/console`, which has no driver here).
const TTYAUX_MAJOR: u32 = 5;

/// The device nodes `devtmpfs` would create for the drivers here: name, minor numbers, mode.
const NODES: [(&str, Device, u32); 6] = [
    ("null", dev(MEM_MAJOR, 3), 0o666),
    ("zero", dev(MEM_MAJOR, 5), 0o666),
    ("full", dev(MEM_MAJOR, 7), 0o666),
    ("random", dev(MEM_MAJOR, 8), 0o666),
    ("urandom", dev(MEM_MAJOR, 9), 0o666),
    ("tty", dev(TTYAUX_MAJOR, 0), 0o666),
];

const fn dev(major: u32, minor: u32) -> Device {
    Device { major, minor }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mem {
    Null,
    Zero,
    Full,
}

/// An open memory device. It keeps the VFS file, so the node sees the open and the close.
struct MemDevice {
    kind: Mem,
    _file: File,
}

/// An open `/dev/random` or `/dev/urandom`. The host's entropy is always ready, so both read
/// without blocking, as `/dev/random` does once the pool is initialised.
struct RandomDevice {
    host: Arc<dyn Host>,
    _file: File,
}

impl Kernel {
    /// `chrdev_open`: the driver the node's device number names; `ENXIO` if there is none. A
    /// block device has no driver at all.
    pub(crate) async fn open_device(&self, file: File) -> SysResult<Arc<dyn FileObject>> {
        if file.file_type() != FileType::CharDevice {
            return Err(Errno::ENXIO.into());
        }
        let rdev = file.node().getattr().await?.rdev;
        let object: Arc<dyn FileObject> = match (rdev.major, rdev.minor) {
            (MEM_MAJOR, 3) => mem(Mem::Null, file),
            (MEM_MAJOR, 5) => mem(Mem::Zero, file),
            (MEM_MAJOR, 7) => mem(Mem::Full, file),
            (MEM_MAJOR, 8 | 9) => Arc::new(RandomDevice {
                host: self.host.clone(),
                _file: file,
            }),
            // `tty_open_current_tty`: no process has a controlling terminal.
            _ => return Err(Errno::ENXIO.into()),
        };
        Ok(object)
    }

    /// Creates the nodes of the devices the kernel has in the directory `at` (resolved from the
    /// root, which must exist), as devtmpfs populates `/dev`: owned by root, with Linux's
    /// numbers and modes.
    pub async fn create_device_nodes(&self, at: &[u8]) -> FsResult<()> {
        create_nodes(&self.vfs, at).await
    }
}

async fn create_nodes(vfs: &Vfs, at: &[u8]) -> FsResult<()> {
    let cred = Cred::root();
    let root = vfs.root();
    let cx = Ctx {
        cred: &cred,
        root: &root,
        umask: 0,
        task: 0,
    };
    let dir = vfs
        .lookup(&cx, &root, at, wasixv2_vfs::LookupFlags::follow())
        .await?;
    for (name, device, mode) in NODES {
        let mode = FileType::CharDevice.mode_bits() | mode;
        match vfs.mknod(&cx, &dir, name.as_bytes(), mode, device).await {
            Ok(()) | Err(Errno::EEXIST) => {}
            Err(errno) => return Err(errno),
        }
    }
    Ok(())
}

fn mem(kind: Mem, file: File) -> Arc<dyn FileObject> {
    Arc::new(MemDevice { kind, _file: file })
}

impl Pollable for MemDevice {
    fn poll(&self, _: Option<&mut PollTable<'_>>) -> PollMask {
        PollMask::DEFAULT
    }
}

impl FileObject for MemDevice {
    /// `read_null` is at end of file; `read_iter_zero` fills the buffer, as `/dev/full` does.
    fn read<'a>(&'a self, _: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        let n = match self.kind {
            Mem::Null => 0,
            Mem::Zero | Mem::Full => {
                buf.fill(0);
                buf.len()
            }
        };
        Box::pin(async move { Ok(n) })
    }

    /// `write_null` takes everything; `write_full` has no space.
    fn write<'a>(&'a self, _: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        let result = match self.kind {
            Mem::Null | Mem::Zero => Ok(data.len()),
            Mem::Full => Err(Errno::ENOSPC.into()),
        };
        Box::pin(async move { result })
    }

    /// The memory devices have no `poll`, so epoll refuses them.
    fn can_poll(&self) -> bool {
        false
    }

    fn positional(&self) -> bool {
        true
    }

    /// `null_lseek`: every seek lands at 0.
    fn llseek(&self, _: u64, _: SeekBase, _: i64) -> Result<u64, Errno> {
        Ok(0)
    }
}

impl Pollable for RandomDevice {
    /// `random_poll` once the pool is ready: always readable and writable.
    fn poll(&self, _: Option<&mut PollTable<'_>>) -> PollMask {
        PollMask::IN | PollMask::RDNORM | PollMask::OUT | PollMask::WRNORM
    }
}

impl FileObject for RandomDevice {
    fn read<'a>(&'a self, _: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        self.host.random(buf);
        Box::pin(async move { Ok(buf.len()) })
    }

    /// Writes mix into the pool without crediting entropy; the host's pool takes nothing.
    fn write<'a>(&'a self, _: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move { Ok(data.len()) })
    }

    fn positional(&self) -> bool {
        true
    }

    /// `noop_llseek`: the position stays where it is.
    fn llseek(&self, pos: u64, _: SeekBase, _: i64) -> Result<u64, Errno> {
        Ok(pos)
    }
}
