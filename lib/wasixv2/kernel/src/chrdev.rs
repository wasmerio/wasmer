//! Character devices: the drivers behind device nodes, found by device number when a node is
//! opened (`chrdev_open`). The memory devices follow `drivers/char/mem.c`, the random devices
//! `drivers/char/random.c`; `/dev/tty` is the opener's controlling terminal (`tty_open`), and
//! the host's console is a terminal of its own (see `tty`). `/dev/kmsg` is the kernel log
//! (see `printk`).
//!
//! The nodes themselves live on an ordinary filesystem, as on Linux, where devtmpfs is a tmpfs:
//! [`Kernel::create_device_nodes`] makes the standard ones.

use std::sync::Arc;

use wasixv2_abi::OpenFlags;
use wasixv2_host::Host;
use wasixv2_syscalls::{Caller, Errno, LocalFuture, SysResult};
use wasixv2_vfs::{Cap, Cred, Ctx, Device, File, FileType, FsResult, Vfs};
use wasixv2_wait::{PollMask, PollTable, Pollable};

use crate::{
    Kernel,
    fasync::{Fasync, FasyncList},
    file::{FileObject, Io, SeekBase},
    sync::MutexExt,
};

/// `MEM_MAJOR`, with `/dev/null`, `/dev/zero`, `/dev/full`, `/dev/random` and `/dev/urandom`.
const MEM_MAJOR: u32 = 1;
/// `TTYAUX_MAJOR`, with `/dev/tty` (and `/dev/console`, which has no driver here).
const TTYAUX_MAJOR: u32 = 5;
/// `MISC_MAJOR`, with `/dev/vsock` at the first dynamic minor (`misc_register` gives dynamic
/// minors from 256 up).
const MISC_MAJOR: u32 = 10;
const VSOCK_MINOR: u32 = 256;
/// The mode of a terminal's node once its user logged in on it (`login`'s `chmod 0620`).
const TTY_MODE: u32 = 0o620;

/// The device nodes `devtmpfs` would create for the drivers here: name, minor numbers, mode.
const NODES: [(&str, Device, u32); 10] = [
    ("null", dev(MEM_MAJOR, 3), 0o666),
    ("zero", dev(MEM_MAJOR, 5), 0o666),
    ("full", dev(MEM_MAJOR, 7), 0o666),
    ("random", dev(MEM_MAJOR, 8), 0o666),
    ("urandom", dev(MEM_MAJOR, 9), 0o666),
    ("kmsg", dev(MEM_MAJOR, 11), 0o644),
    ("tty", dev(TTYAUX_MAJOR, 0), 0o666),
    ("ptmx", crate::tty::PTMX, 0o666),
    ("vsock", dev(MISC_MAJOR, VSOCK_MINOR), 0o666),
    ("fuse", dev(MISC_MAJOR, 229), 0o666),
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
    /// The files that asked for I/O signals (`random_fasync`). Linux signals them once, when
    /// the pool becomes ready, which it is here from the start.
    fasync: FasyncList,
}

impl Kernel {
    /// `chrdev_open`: the driver the node's device number names; `ENXIO` if there is none. A
    /// block device has no driver at all. `flags` are the open's.
    pub(crate) async fn open_device(
        &self,
        caller: &Caller<'_>,
        file: File,
        flags: OpenFlags,
    ) -> SysResult<Arc<dyn FileObject>> {
        if file.file_type() != FileType::CharDevice {
            return Err(Errno::ENXIO.into());
        }
        let rdev = file.node().getattr().await?.rdev;
        if let Some(tty) = self.open_tty(caller, &file, rdev, flags)? {
            return Ok(tty);
        }
        let object: Arc<dyn FileObject> = match (rdev.major, rdev.minor) {
            (MEM_MAJOR, 3) => mem(Mem::Null, file),
            (MEM_MAJOR, 5) => mem(Mem::Zero, file),
            (MEM_MAJOR, 7) => mem(Mem::Full, file),
            (MEM_MAJOR, 8 | 9) => Arc::new(RandomDevice {
                host: self.host.clone(),
                _file: file,
                fasync: FasyncList::default(),
            }),
            (MEM_MAJOR, 11) => self.open_dev_kmsg(caller, file, flags)?,
            (MISC_MAJOR, 229) => self.open_fuse(caller.task()),
            (MISC_MAJOR, VSOCK_MINOR) => Arc::new(crate::net::vsock::VsockDevice {
                transports: self.vsock_transports(),
            }),
            _ => return Err(Errno::ENXIO.into()),
        };
        Ok(object)
    }

    /// `tty_open` for the terminal devices: `/dev/tty` is the caller's controlling terminal
    /// (`ENXIO` without one), the console is itself, `/dev/ptmx` makes a pty and opens its
    /// master, and a pty slave opens through its node in the pty filesystem (`EIO` through
    /// any other). Opening a terminal without `O_NOCTTY` gives a session leader without one
    /// its controlling terminal; a pty master never becomes one. `None` for other devices.
    fn open_tty(
        &self,
        caller: &Caller<'_>,
        file: &File,
        rdev: Device,
        flags: OpenFlags,
    ) -> SysResult<Option<Arc<dyn FileObject>>> {
        if rdev == crate::tty::PTMX {
            return Ok(Some(Arc::new(self.open_ptmx(caller)?)));
        }
        let process = self.process(caller.task());
        let (tty, noctty) = match (rdev.major, rdev.minor) {
            // `tty_open_current_tty`.
            (TTYAUX_MAJOR, 0) => {
                let tty = process.ident.lock_or_recover().tty.clone();
                (tty.ok_or(Errno::ENXIO)?, true)
            }
            (crate::tty::HVC_MAJOR, 0) => match self.console() {
                Some(tty) => (tty.clone(), flags.contains(OpenFlags::O_NOCTTY)),
                // `hvc_open` of a console nobody provides.
                None => return Err(Errno::ENODEV.into()),
            },
            // `pts_unix98_lookup`.
            (crate::tty::PTY_SLAVE_MAJOR, _) => {
                let slave = self.ptys.slave(&**file.node()).ok_or(Errno::EIO)?;
                (slave, flags.contains(OpenFlags::O_NOCTTY))
            }
            _ => return Ok(None),
        };
        // `tty_reopen`: `capable(CAP_SYS_ADMIN)`.
        let admin = self.cred(caller).capable_init(Cap::SysAdmin);
        let file = tty.open(admin)?;
        if !noctty {
            let readable = matches!(
                flags & OpenFlags::O_ACCMODE,
                OpenFlags::O_RDONLY | OpenFlags::O_RDWR
            );
            tty.open_set_ctty(&process, readable);
        }
        Ok(Some(Arc::new(file)))
    }

    /// Creates the nodes of the devices the kernel has in the directory `at` (resolved from the
    /// root, which must exist), as devtmpfs populates `/dev`: owned by root, with Linux's
    /// numbers and modes.
    pub async fn create_device_nodes(&self, at: &[u8]) -> FsResult<()> {
        create_nodes(&self.vfs, at).await?;
        if let Some((name, device)) = self.console_node(at) {
            // The console's user is logged in on it: the node is theirs, as `login` leaves a
            // terminal.
            let owner = (self.credentials.uid(), self.credentials.gid());
            create_tty_node(&self.vfs, at, name, device, owner).await?;
        }
        Ok(())
    }
}

/// Makes the node of the terminal `device` named `name` in `at`, owned by `owner`.
async fn create_tty_node(
    vfs: &Vfs,
    at: &[u8],
    name: &str,
    device: Device,
    owner: (u32, u32),
) -> FsResult<()> {
    let cred = Cred::root();
    let root = vfs.root();
    let cx = Ctx {
        cred: &cred,
        root: &root,
        umask: 0,
        task: 0,
        leases: None,
    };
    let dir = vfs
        .lookup(&cx, &root, at, wasixv2_vfs::LookupFlags::follow())
        .await?;
    let mode = FileType::CharDevice.mode_bits() | TTY_MODE;
    match vfs.mknod(&cx, &dir, name.as_bytes(), mode, device).await {
        Ok(()) | Err(Errno::EEXIST) => {}
        Err(errno) => return Err(errno),
    }
    let node = vfs
        .lookup(
            &cx,
            &dir,
            name.as_bytes(),
            wasixv2_vfs::LookupFlags::default(),
        )
        .await?;
    vfs.chown(&cx, &node, Some(owner.0), Some(owner.1)).await
}

async fn create_nodes(vfs: &Vfs, at: &[u8]) -> FsResult<()> {
    let cred = Cred::root();
    let root = vfs.root();
    let cx = Ctx {
        cred: &cred,
        root: &root,
        umask: 0,
        task: 0,
        leases: None,
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

    fn fills_reads(&self) -> bool {
        true
    }

    /// `copy_splice_read` for `zero` and `full`; `null` has none.
    fn splice_read(&self) -> bool {
        !matches!(self.kind, Mem::Null)
    }

    /// `splice_write_null` and `splice_write_zero`; `full` has none.
    fn splice_write(&self) -> bool {
        !matches!(self.kind, Mem::Full)
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

    fn fills_reads(&self) -> bool {
        true
    }

    /// `random_fasync`.
    fn fasync(&self, fasync: &Fasync<'_>) -> Option<Result<(), Errno>> {
        self.fasync.helper(fasync);
        Some(Ok(()))
    }

    /// `copy_splice_read`.
    fn splice_read(&self) -> bool {
        true
    }

    /// `iter_file_splice_write`.
    fn splice_write(&self) -> bool {
        true
    }

    /// `noop_llseek`: the position stays where it is.
    fn llseek(&self, pos: u64, _: SeekBase, _: i64) -> Result<u64, Errno> {
        Ok(pos)
    }
}
