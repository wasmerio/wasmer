//! Pseudo-terminals: a pair from `/dev/ptmx` with its slave's node in `/dev/pts`, the bytes
//! between master and slave through the slave's line discipline, the master's `ioctl`s on the
//! slave, packet mode, and what either side's close does to the other.

use wasixv2_abi::{IoctlRequest, OpenFlags, Pid, Signal, Termios, TermiosLflag, Winsize};
use wasixv2_syscalls::{read_value, write_value};
use wasixv2_vfs::MountFlags;
use wasixv2_wait::{PollMask, Pollable};

use super::{ipc::errno, proc::ForkableMemory, *};

/// A machine with `/dev`, `/dev/pts` and `/proc`, and its first process, a session leader
/// without a controlling terminal.
struct Machine {
    kernel: Kernel,
    leader: TaskId,
}

const RW_NOCTTY: OpenFlags = OpenFlags(OpenFlags::O_RDWR.0 | OpenFlags::O_NOCTTY.0);

fn machine() -> Machine {
    let host: Arc<dyn Host> = Arc::new(TestHost);
    let clock = Arc::new(HostClock(host.clone()));
    let root = MemFs::new(clock.clone(), 0o755, 1, 1);
    let shm = MemFs::new(clock.clone(), 0o1777, 0, 0);
    let kernel = Kernel::new(host, Arc::new(root))
        .with_shm(Arc::new(shm))
        .with_init();
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    let dev = MemFs::new(clock, 0o755, 0, 0);
    block_on(vfs.mount(b"/dev", Arc::new(dev), MountFlags::default(), true)).unwrap();
    block_on(kernel.create_device_nodes(b"/dev")).unwrap();
    block_on(vfs.mount(b"/dev/pts", kernel.devpts(), MountFlags::default(), true)).unwrap();
    let stdin = Arc::new(Input(Mutex::new(Vec::new())));
    let (kernel, leader, _) = spawn_on(kernel, stdin, &[], &[]);
    Machine { kernel, leader }
}

fn memory() -> ForkableMemory {
    ForkableMemory(FlatMemory(RefCell::new(vec![0; 65536])))
}

fn blocked(sigs: &[Signal]) -> u64 {
    sigs.iter().map(|sig| 1 << (sig.0 - 1)).sum()
}

impl Machine {
    fn open(&self, caller: &Caller<'_>, path: &str, flags: OpenFlags) -> SysResult<Fd> {
        let mut name = path.as_bytes().to_vec();
        name.push(0);
        block_on(caller.memory().write(0x800, &name)).unwrap();
        let open = self
            .kernel
            .openat(caller, Fd(-100), GuestPtr::new(0x800), flags, 0);
        block_on(open)
    }

    fn close(&self, caller: &Caller<'_>, fd: Fd) {
        assert_eq!(block_on(self.kernel.close(caller, fd)), Ok(0));
    }

    fn ioctl(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        request: IoctlRequest,
        arg: u32,
    ) -> SysResult<i32> {
        block_on(self.kernel.ioctl(caller, fd, request, arg))
    }

    /// An `int` result of `request`, through memory at 0x10.
    fn get_int(&self, caller: &Caller<'_>, fd: Fd, request: IoctlRequest) -> SysResult<i32> {
        self.ioctl(caller, fd, request, 0x10)?;
        Ok(block_on(read_value(caller.memory(), GuestPtr::new(0x10))).unwrap())
    }

    /// `request` with a pointer to the `int` `value`.
    fn set_int(&self, caller: &Caller<'_>, fd: Fd, request: IoctlRequest, value: i32) {
        block_on(write_value(caller.memory(), GuestPtrMut::new(0x10), &value)).unwrap();
        assert_eq!(self.ioctl(caller, fd, request, 0x10), Ok(0));
    }

    fn tcgetattr(&self, caller: &Caller<'_>, fd: Fd) -> Termios {
        self.ioctl(caller, fd, IoctlRequest::TCGETS, 0x40).unwrap();
        block_on(read_value(caller.memory(), GuestPtr::new(0x40))).unwrap()
    }

    fn tcsetattr(&self, caller: &Caller<'_>, fd: Fd, termios: &Termios) {
        block_on(write_value(
            caller.memory(),
            GuestPtrMut::new(0x40),
            termios,
        ))
        .unwrap();
        assert_eq!(self.ioctl(caller, fd, IoctlRequest::TCSETS, 0x40), Ok(0));
    }

    fn read(&self, caller: &Caller<'_>, fd: Fd, len: u32) -> SysResult<Vec<u8>> {
        let n = block_on(self.kernel.read(caller, fd, UserSliceMut::new(0x2000, len)))?;
        let mut buf = vec![0; n as usize];
        block_on(caller.memory().read(0x2000, &mut buf)).unwrap();
        Ok(buf)
    }

    fn write(&self, caller: &Caller<'_>, fd: Fd, data: &[u8]) -> SysResult<u32> {
        block_on(caller.memory().write(0x2000, data)).unwrap();
        let len = data.len() as u32;
        block_on(self.kernel.write(caller, fd, UserSlice::new(0x2000, len)))
    }

    fn poll(&self, task: TaskId, fd: Fd) -> PollMask {
        Pollable::poll(&*self.kernel.file(task, fd).unwrap(), None)
    }

    /// `posix_openpt`, `unlockpt` and the slave opened with `flags`.
    fn openpty(&self, caller: &Caller<'_>, flags: OpenFlags) -> (Fd, Fd) {
        let master = self.open(caller, "/dev/ptmx", RW_NOCTTY).unwrap();
        self.set_int(caller, master, IoctlRequest::TIOCSPTLCK, 0);
        let index = self
            .get_int(caller, master, IoctlRequest::TIOCGPTN)
            .unwrap();
        let slave = self.open(caller, &format!("/dev/pts/{index}"), flags);
        (master, slave.unwrap())
    }

    fn fork(&self, caller: &Caller<'_>) -> TaskId {
        let flags = Signal::SIGCHLD.0 as u32;
        let clone = self
            .kernel
            .clone(caller, flags, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL);
        let child = TaskId(block_on(clone).expect("fork") as u32);
        while self.kernel.take_pending(caller.task()).is_some() {}
        child
    }

    fn pending(&self, task: TaskId, sig: Signal) -> bool {
        self.kernel.task_signals(task).group().is_pending(sig.0)
    }
}

/// `/dev/ptmx` makes a locked pair; its slave is `/dev/pts/N` until the master closes, and
/// its index is free again once neither side is open.
#[test]
fn ptmx_makes_a_locked_pair_whose_slave_is_in_devpts() {
    let m = machine();
    let memory = memory();
    let caller = Caller::new(m.leader, &memory);
    let master = m.open(&caller, "/dev/ptmx", RW_NOCTTY).unwrap();
    assert_eq!(m.get_int(&caller, master, IoctlRequest::TIOCGPTN), Ok(0));
    assert_eq!(m.get_int(&caller, master, IoctlRequest::TIOCGPTLCK), Ok(1));
    assert_eq!(errno(m.open(&caller, "/dev/pts/0", RW_NOCTTY)), Errno::EIO);

    m.set_int(&caller, master, IoctlRequest::TIOCSPTLCK, 0);
    let slave = m.open(&caller, "/dev/pts/0", RW_NOCTTY).unwrap();
    let dev = crate::tty::encode_dev(136, 0) as i32;
    assert_eq!(m.get_int(&caller, slave, IoctlRequest::TIOCGDEV), Ok(dev));
    // The pty requests are the master's.
    assert_eq!(
        errno(m.get_int(&caller, slave, IoctlRequest::TIOCGPTN)),
        Errno::ENOTTY
    );
    let second = m.open(&caller, "/dev/ptmx", RW_NOCTTY).unwrap();
    assert_eq!(m.get_int(&caller, second, IoctlRequest::TIOCGPTN), Ok(1));

    m.close(&caller, master);
    assert_eq!(
        errno(m.open(&caller, "/dev/pts/0", RW_NOCTTY)),
        Errno::ENOENT
    );
    // Index 0 is taken while its slave is open.
    let third = m.open(&caller, "/dev/ptmx", RW_NOCTTY).unwrap();
    assert_eq!(m.get_int(&caller, third, IoctlRequest::TIOCGPTN), Ok(2));
    m.close(&caller, slave);
    let fourth = m.open(&caller, "/dev/ptmx", RW_NOCTTY).unwrap();
    assert_eq!(m.get_int(&caller, fourth, IoctlRequest::TIOCGPTN), Ok(0));
}

/// What the master writes is the slave's input, edited and echoed; what the slave writes
/// reaches the master post-processed.
#[test]
fn master_and_slave_talk_through_the_slaves_discipline() {
    let m = machine();
    let memory = memory();
    let caller = Caller::new(m.leader, &memory);
    let (master, slave) = m.openpty(&caller, RW_NOCTTY);

    assert_eq!(m.write(&caller, master, b"hx\x7fi\r"), Ok(5));
    assert_eq!(m.read(&caller, slave, 64), Ok(b"hi\n".to_vec()));
    assert_eq!(
        m.read(&caller, master, 64),
        Ok(b"hx\x08 \x08i\r\n".to_vec())
    );

    assert_eq!(m.write(&caller, slave, b"a\nb"), Ok(3));
    assert_eq!(m.read(&caller, master, 64), Ok(b"a\r\nb".to_vec()));
}

/// Termios, window size and device `ioctl`s on the master are the slave's.
#[test]
fn the_masters_terminal_ioctls_act_on_the_slave() {
    let m = machine();
    let memory = memory();
    let caller = Caller::new(m.leader, &memory);
    let (master, slave) = m.openpty(&caller, RW_NOCTTY);

    let mut termios = m.tcgetattr(&caller, master);
    assert_eq!(termios, m.tcgetattr(&caller, slave));
    termios.c_lflag.0 &= !TermiosLflag::ECHO.0;
    m.tcsetattr(&caller, master, &termios);
    assert_eq!(m.tcgetattr(&caller, slave).c_lflag, termios.c_lflag);
    assert_eq!(m.write(&caller, master, b"quiet\n"), Ok(6));
    assert_eq!(m.get_int(&caller, master, IoctlRequest::FIONREAD), Ok(0));
    assert_eq!(m.get_int(&caller, slave, IoctlRequest::FIONREAD), Ok(6));

    let winsize = Winsize {
        ws_row: 30,
        ws_col: 100,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    block_on(write_value(&memory, GuestPtrMut::new(0x20), &winsize)).unwrap();
    assert_eq!(
        m.ioctl(&caller, master, IoctlRequest::TIOCSWINSZ, 0x20),
        Ok(0)
    );
    assert_eq!(
        m.ioctl(&caller, slave, IoctlRequest::TIOCGWINSZ, 0x30),
        Ok(0)
    );
    let got: Winsize = block_on(read_value(&memory, GuestPtr::new(0x30))).unwrap();
    assert_eq!(got, winsize);
    let dev = crate::tty::encode_dev(136, 0) as i32;
    assert_eq!(m.get_int(&caller, master, IoctlRequest::TIOCGDEV), Ok(dev));
}

/// `TCGETA` and `TCSETA` see the low 16 bits of each flag word and keep the rest.
#[test]
fn termio_ioctls_take_the_low_flag_bits() {
    let m = machine();
    let memory = memory();
    let caller = Caller::new(m.leader, &memory);
    let (_master, slave) = m.openpty(&caller, RW_NOCTTY);
    let termios = m.tcgetattr(&caller, slave);

    assert_eq!(m.ioctl(&caller, slave, IoctlRequest::TCGETA, 0x100), Ok(0));
    let mut termio = vec![0; 17];
    block_on(memory.read(0x100, &mut termio)).unwrap();
    let lflag = u16::from_le_bytes([termio[6], termio[7]]);
    assert_eq!(u32::from(lflag), termios.c_lflag.0 & 0xffff);
    assert_eq!(termio[9..], termios.c_cc[..8]);

    let echo = TermiosLflag::ECHO.0 as u16;
    termio[6..8].copy_from_slice(&(lflag & !echo).to_le_bytes());
    block_on(memory.write(0x100, &termio)).unwrap();
    assert_eq!(m.ioctl(&caller, slave, IoctlRequest::TCSETAW, 0x100), Ok(0));
    let after = m.tcgetattr(&caller, slave);
    assert_eq!(after.c_lflag.0, termios.c_lflag.0 & !TermiosLflag::ECHO.0);
}

/// A session leader acquires the slave it opens; the master never becomes a controlling
/// terminal, and tells the slave's foreground group and session to anyone.
#[test]
fn a_session_leader_acquires_the_slave_not_the_master() {
    let m = machine();
    let memory = memory();
    let caller = Caller::new(m.leader, &memory);
    let master = m.open(&caller, "/dev/ptmx", OpenFlags::O_RDWR).unwrap();
    assert_eq!(
        errno(m.open(&caller, "/dev/tty", OpenFlags::O_RDWR)),
        Errno::ENXIO
    );
    assert_eq!(
        errno(m.ioctl(&caller, master, IoctlRequest::TIOCNOTTY, 0)),
        Errno::ENOTTY
    );
    m.set_int(&caller, master, IoctlRequest::TIOCSPTLCK, 0);
    // No session has the slave yet.
    assert_eq!(m.get_int(&caller, master, IoctlRequest::TIOCGPGRP), Ok(0));
    assert_eq!(
        errno(m.get_int(&caller, master, IoctlRequest::TIOCGSID)),
        Errno::ENOTTY
    );

    let child = m.fork(&caller);
    let child_memory = self::memory();
    let child_caller = Caller::new(child, &child_memory);
    assert_eq!(block_on(m.kernel.setsid(&child_caller)), Ok(child.0 as Pid));
    let slave = m
        .open(&child_caller, "/dev/pts/0", OpenFlags::O_RDWR)
        .unwrap();
    let pid = child.0 as i32;
    assert_eq!(
        m.get_int(&child_caller, slave, IoctlRequest::TIOCGPGRP),
        Ok(pid)
    );
    assert!(m.open(&child_caller, "/dev/tty", OpenFlags::O_RDWR).is_ok());

    assert_eq!(m.get_int(&caller, master, IoctlRequest::TIOCGPGRP), Ok(pid));
    assert_eq!(m.get_int(&caller, master, IoctlRequest::TIOCGSID), Ok(pid));
    // The slave is not the parent's: it asks through the slave in vain.
    let parents = m.open(&caller, "/dev/pts/0", RW_NOCTTY).unwrap();
    assert_eq!(
        errno(m.get_int(&caller, parents, IoctlRequest::TIOCGPGRP)),
        Errno::ENOTTY
    );
}

/// `TIOCSIG` and the signal characters written to the master signal the slave's foreground
/// process group; only the signal characters' signals go.
#[test]
fn the_master_signals_the_slaves_foreground_group() {
    let m = machine();
    let memory = memory();
    let caller = Caller::new(m.leader, &memory);
    let master = m.open(&caller, "/dev/ptmx", RW_NOCTTY).unwrap();
    m.set_int(&caller, master, IoctlRequest::TIOCSPTLCK, 0);
    let child = m.fork(&caller);
    let child_memory = self::memory();
    let child_caller = Caller::new(child, &child_memory);
    assert_eq!(block_on(m.kernel.setsid(&child_caller)), Ok(child.0 as Pid));
    let slave = m
        .open(&child_caller, "/dev/pts/0", OpenFlags::O_RDWR)
        .unwrap();
    m.kernel
        .task_signals(child)
        .replace_blocked(blocked(&[Signal::SIGINT, Signal::SIGQUIT]));

    let sigint = Signal::SIGINT.0 as u32;
    assert_eq!(
        m.ioctl(&caller, master, IoctlRequest::TIOCSIG, sigint),
        Ok(0)
    );
    assert!(m.pending(child, Signal::SIGINT));
    let sigkill = Signal::SIGKILL.0 as u32;
    assert_eq!(
        errno(m.ioctl(&caller, master, IoctlRequest::TIOCSIG, sigkill)),
        Errno::EINVAL
    );
    assert_eq!(
        errno(m.ioctl(&child_caller, slave, IoctlRequest::TIOCSIG, sigint)),
        Errno::ENOTTY
    );
    assert_eq!(m.write(&caller, master, b"\x1c"), Ok(1));
    assert!(m.pending(child, Signal::SIGQUIT));
    assert_eq!(m.read(&caller, master, 16), Ok(b"^\\".to_vec()));
}

/// In packet mode a master read is `TIOCPKT_DATA` and data, or one status byte: the slave's
/// input or output flushed, its output stopped or started. Status shows as `POLLPRI`.
#[test]
fn packet_mode_reports_the_slaves_flushes_and_flow() {
    let m = machine();
    let memory = memory();
    let caller = Caller::new(m.leader, &memory);
    let (master, slave) = m.openpty(&caller, RW_NOCTTY);
    m.set_int(&caller, master, IoctlRequest::TIOCPKT, 1);
    assert_eq!(m.get_int(&caller, master, IoctlRequest::TIOCGPKT), Ok(1));

    assert_eq!(m.write(&caller, slave, b"x"), Ok(1));
    assert_eq!(m.read(&caller, master, 16), Ok(vec![0, b'x']));

    let tciflush = 0;
    assert_eq!(
        m.ioctl(&caller, slave, IoctlRequest::TCFLSH, tciflush),
        Ok(0)
    );
    assert!(m.poll(m.leader, master).contains(PollMask::PRI));
    assert_eq!(m.read(&caller, master, 16), Ok(vec![1]));
    assert!(!m.poll(m.leader, master).contains(PollMask::PRI));

    let tcoflush = 1;
    assert_eq!(
        m.ioctl(&caller, slave, IoctlRequest::TCFLSH, tcoflush),
        Ok(0)
    );
    assert_eq!(m.read(&caller, master, 16), Ok(vec![2]));

    // `^S` and `^Q` from the master stop and start the slave's output.
    assert_eq!(m.write(&caller, master, b"\x13"), Ok(1));
    assert_eq!(m.read(&caller, master, 16), Ok(vec![4]));
    assert_eq!(m.write(&caller, master, b"\x11"), Ok(1));
    assert_eq!(m.read(&caller, master, 16), Ok(vec![8]));

    // Turning `IXON` off says flow control is off.
    let mut termios = m.tcgetattr(&caller, slave);
    termios.c_iflag.0 &= !wasixv2_abi::TermiosIflag::IXON.0;
    m.tcsetattr(&caller, slave, &termios);
    assert_eq!(m.read(&caller, master, 16), Ok(vec![0x10]));
}

/// The master's close hangs the slave up: the session the slave controls loses it and its
/// leader gets SIGHUP and SIGCONT, reads end, writes fail, and the node is gone.
#[test]
fn the_masters_close_hangs_up_the_slave() {
    let m = machine();
    let memory = memory();
    let caller = Caller::new(m.leader, &memory);
    let master = m.open(&caller, "/dev/ptmx", RW_NOCTTY).unwrap();
    m.set_int(&caller, master, IoctlRequest::TIOCSPTLCK, 0);
    let child = m.fork(&caller);
    let child_memory = self::memory();
    let child_caller = Caller::new(child, &child_memory);
    m.close(&child_caller, master);
    assert_eq!(block_on(m.kernel.setsid(&child_caller)), Ok(child.0 as Pid));
    let slave = m
        .open(&child_caller, "/dev/pts/0", OpenFlags::O_RDWR)
        .unwrap();
    m.kernel
        .task_signals(child)
        .replace_blocked(blocked(&[Signal::SIGHUP, Signal::SIGCONT]));

    m.close(&caller, master);

    assert!(m.pending(child, Signal::SIGHUP));
    assert!(m.pending(child, Signal::SIGCONT));
    assert_eq!(m.read(&child_caller, slave, 16), Ok(Vec::new()));
    assert_eq!(errno(m.write(&child_caller, slave, b"x")), Errno::EIO);
    assert!(m.poll(child, slave).contains(PollMask::HUP));
    assert_eq!(
        errno(m.open(&child_caller, "/dev/pts/0", RW_NOCTTY)),
        Errno::ENOENT
    );
    assert_eq!(
        errno(m.open(&child_caller, "/dev/tty", OpenFlags::O_RDWR)),
        Errno::ENXIO
    );
}

/// Once the slave's last file closes, the master reads what the slave wrote, then `EIO`, and
/// polls as hung up; opening the slave again makes it a reader that waits.
#[test]
fn the_slaves_last_close_ends_the_masters_reads() {
    let m = machine();
    let memory = memory();
    let caller = Caller::new(m.leader, &memory);
    let (master, slave) = m.openpty(&caller, RW_NOCTTY);
    assert_eq!(m.write(&caller, slave, b"bye"), Ok(3));
    m.close(&caller, slave);

    assert!(m.poll(m.leader, master).contains(PollMask::HUP));
    assert_eq!(m.read(&caller, master, 16), Ok(b"bye".to_vec()));
    assert_eq!(errno(m.read(&caller, master, 16)), Errno::EIO);

    let _slave = m.open(&caller, "/dev/pts/0", RW_NOCTTY).unwrap();
    assert!(!m.poll(m.leader, master).contains(PollMask::HUP));
    let mut read = Box::pin(
        m.kernel
            .read(&caller, master, UserSliceMut::new(0x2000, 16)),
    );
    assert_eq!(futures::FutureExt::now_or_never(read.as_mut()), None);
}

/// A side whose output the other does not read takes no more once the buffer between them is
/// full; the other side's read makes room.
#[test]
fn a_full_pty_holds_the_writer_until_the_other_side_reads() {
    let m = machine();
    let memory = memory();
    let caller = Caller::new(m.leader, &memory);
    let nonblock = RW_NOCTTY | OpenFlags::O_NONBLOCK;
    let (master, slave) = m.openpty(&caller, nonblock);
    let data = vec![b'z'; 32768];

    let taken = m.write(&caller, slave, &data).unwrap() as usize;
    assert!(taken > 8192 && taken < data.len(), "{taken}");
    assert_eq!(errno(m.write(&caller, slave, b"z")), Errno::EAGAIN);
    assert!(!m.poll(m.leader, slave).contains(PollMask::OUT));

    let mut read = 0;
    while read < taken {
        read += m.read(&caller, master, 8192).unwrap().len();
    }
    assert_eq!(read, taken);
    assert!(m.poll(m.leader, slave).contains(PollMask::OUT));
    assert_eq!(m.write(&caller, slave, b"z"), Ok(1));
}

/// `TIOCGPTPEER` opens the master's slave, as its node does: not while locked, and only from
/// the master.
#[test]
fn tiocgptpeer_opens_the_slave() {
    let m = machine();
    let memory = memory();
    let caller = Caller::new(m.leader, &memory);
    let master = m.open(&caller, "/dev/ptmx", RW_NOCTTY).unwrap();
    let flags = RW_NOCTTY.0;
    assert_eq!(
        errno(m.ioctl(&caller, master, IoctlRequest::TIOCGPTPEER, flags)),
        Errno::EIO
    );
    m.set_int(&caller, master, IoctlRequest::TIOCSPTLCK, 0);
    let peer = Fd(m
        .ioctl(&caller, master, IoctlRequest::TIOCGPTPEER, flags)
        .unwrap());
    let dev = crate::tty::encode_dev(136, 0) as i32;
    assert_eq!(m.get_int(&caller, peer, IoctlRequest::TIOCGDEV), Ok(dev));
    assert_eq!(m.write(&caller, peer, b"p"), Ok(1));
    assert_eq!(m.read(&caller, master, 16), Ok(b"p".to_vec()));
    assert_eq!(
        errno(m.ioctl(&caller, peer, IoctlRequest::TIOCGPTPEER, flags)),
        Errno::EIO
    );
}
