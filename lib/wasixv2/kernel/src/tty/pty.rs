//! Pseudo-terminals: Linux's Unix98 ptys (`drivers/tty/pty.c`).
//!
//! Opening `/dev/ptmx` makes a pair: the master, whose holder plays the device, and the slave,
//! `/dev/pts/N`, a terminal like any other, with job control for the session it controls. Each
//! side is a [`Tty`] with its own line discipline: what one side's discipline sends its device
//! is the other side's input, through a buffer that holds what the receiving discipline has no
//! room for yet (the receiving port's flip buffer, 8 KiB). The master's termios is raw and stays
//! so: termios, window and job control `ioctl`s on the master act on the slave
//! (`tty_pair_get_tty`).
//!
//! The slave is locked when made, and opens only once unlocked (`TIOCSPTLCK`) and while the
//! master is open. When the master's last file closes the slave is hung up and its node goes;
//! when the slave's last file closes, a master read past the remaining output fails with `EIO`.
//! The nodes are in the pty filesystem ([`devpts`](super::devpts)).

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, OnceLock, Weak},
};

use wasixv2_abi::{OpenFlags, Signal, Termios, TermiosCflag, TermiosIflag, TiocpktFlags, Winsize};
use wasixv2_syscalls::{Caller, Errno, SysResult};
use wasixv2_vfs::{Cap, Device, LookupFlags, OpenHow, Path};
use wasixv2_wait::PollMask;

use super::{
    Driver, Tty, TtyFile,
    devpts::{DEVPTS_SUPER_MAGIC, Ptys},
    ldisc,
};
use crate::{Kernel, file::OpenFile, sync::MutexExt};

/// `TTYAUX_MAJOR` 2: `/dev/ptmx`, which makes a new pair when opened.
pub(crate) const PTMX: Device = Device { major: 5, minor: 2 };
/// `UNIX98_PTY_MASTER_MAJOR`: the masters' device numbers, which no node has.
const PTY_MASTER_MAJOR: u32 = 128;
/// `UNIX98_PTY_SLAVE_MAJOR`: the slaves, `/dev/pts/N` being 136:N.
pub(crate) const PTY_SLAVE_MAJOR: u32 = 136;
/// How much a side's buffer holds of what the other sends it, beyond what its discipline took
/// (`tty_buffer_set_limit(port, 8192)` in `pty_common_install`). A write step that starts below
/// the limit is taken whole.
const BUFFER_LIMIT: usize = 8192;

/// One side of a pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Side {
    Master,
    Slave,
}

impl Side {
    fn other(self) -> Self {
        match self {
            Side::Master => Side::Slave,
            Side::Slave => Side::Master,
        }
    }
}

/// What the two sides of a pair share: each other (`tty->link`) and the pair's flags.
pub(crate) struct Pty {
    index: u32,
    master: OnceLock<Weak<Tty>>,
    slave: OnceLock<Weak<Tty>>,
    /// What the master sent that the slave's discipline has not taken yet.
    to_slave: Mutex<VecDeque<u8>>,
    /// What the slave sent that the master's discipline has not taken yet.
    to_master: Mutex<VecDeque<u8>>,
    state: Mutex<State>,
    /// The index and the node live in the pty filesystem's table.
    ptys: Weak<Ptys>,
}

struct State {
    /// `TTY_PTY_LOCK`: the slave does not open. Set when the pair is made.
    locked: bool,
    /// The master's packet mode (`ctrl.packet`).
    packet: bool,
    /// The slave's status for a master in packet mode (`ctrl.pktstatus`), `TIOCPKT_*` bits.
    pktstatus: u8,
    /// The master has its one open file (`TTY_OTHER_CLOSED` on the slave once it closes).
    master_open: bool,
    /// Files open on the slave.
    slave_files: usize,
    /// The slave's last file closed (`TTY_OTHER_CLOSED` on the master), until it opens again.
    slave_closed: bool,
}

/// The master's termios (`ptm_driver->init_termios`): no processing at all.
fn master_termios() -> Termios {
    let mut termios = ldisc::std_termios();
    termios.c_iflag = TermiosIflag(0);
    termios.c_oflag = wasixv2_abi::TermiosOflag(0);
    termios.c_cflag = TermiosCflag(TermiosCflag::CS8.0 | TermiosCflag::CREAD.0 | ldisc::B38400);
    termios.c_lflag = wasixv2_abi::TermiosLflag(0);
    termios
}

/// The slave's termios (`pts_driver->init_termios`): a terminal's defaults, without `HUPCL`.
fn slave_termios() -> Termios {
    let mut termios = ldisc::std_termios();
    termios.c_cflag = TermiosCflag(TermiosCflag::CS8.0 | TermiosCflag::CREAD.0 | ldisc::B38400);
    termios
}

impl Pty {
    pub(crate) fn index(&self) -> u32 {
        self.index
    }

    fn side(&self, side: Side) -> Option<Arc<Tty>> {
        match side {
            Side::Master => self.master.get()?.upgrade(),
            Side::Slave => self.slave.get()?.upgrade(),
        }
    }

    /// What waits to be taken by `side`'s discipline.
    fn buffer(&self, side: Side) -> &Mutex<VecDeque<u8>> {
        match side {
            Side::Master => &self.to_master,
            Side::Slave => &self.to_slave,
        }
    }

    // ---- the device of `from` (`pty_write`, `pty_write_room`, `pty_unthrottle`) ----

    /// Whether `from` may send the other side more (`pty_write_room`).
    pub(super) fn write_room(&self, from: Side) -> bool {
        self.buffer(from.other()).lock_or_recover().len() < BUFFER_LIMIT
    }

    /// `pty_write`: `data` from `from` is the other side's input. Its echoes go back at once,
    /// as `n_tty_receive_buf` flushes them.
    pub(super) async fn write(&self, from: Side, data: &[u8]) {
        let to = from.other();
        self.buffer(to).lock_or_recover().extend(data);
        let Some(peer) = self.side(to) else {
            return;
        };
        self.push(to, &peer);
        // The master echoes nothing, so this goes at most one level deep.
        Box::pin(peer.flush_echoes()).await;
    }

    /// The flip buffer's work (`flush_to_ldisc`): hands `tty`, the side `to`, what waits for
    /// it, as much as its discipline takes; the other side may then send more.
    fn push(&self, to: Side, tty: &Tty) {
        let received = {
            let mut buffer = self.buffer(to).lock_or_recover();
            if buffer.is_empty() {
                return;
            }
            let received = tty
                .ldisc
                .lock_or_recover()
                .receive(buffer.make_contiguous());
            buffer.drain(..received.taken);
            received
        };
        let taken = received.taken;
        tty.received(received);
        if taken > 0
            && let Some(sender) = self.side(to.other())
        {
            sender.wakeup();
        }
    }

    /// `side`'s discipline made room: it takes more of what waits for it, and the other side
    /// may send more (`pty_unthrottle`'s `tty_wakeup` of the link).
    pub(super) fn unthrottle(&self, side: Side) {
        if let Some(tty) = self.side(side) {
            self.push(side, &tty);
        }
        if let Some(sender) = self.side(side.other()) {
            sender.wakeup();
        }
    }

    /// `pty_flush_buffer` for `from`: what it sent that the other side has not taken is gone;
    /// a master in packet mode hears of the slave's (`TIOCPKT_FLUSHWRITE`).
    pub(super) fn flush_buffer(&self, from: Side) {
        self.buffer(from.other()).lock_or_recover().clear();
        if from == Side::Slave {
            self.packet_status(TiocpktFlags::TIOCPKT_FLUSHWRITE.0 as u8, 0);
        }
        if let Some(sender) = self.side(from) {
            sender.wakeup();
        }
    }

    /// `tty_buffer_flush` of `side`'s own buffer: what was sent to it and not taken is gone.
    pub(super) fn flush_incoming(&self, side: Side) {
        self.buffer(side).lock_or_recover().clear();
        if let Some(sender) = self.side(side.other()) {
            sender.wakeup();
        }
    }

    // ---- packet mode ----

    /// Sets `set` and clears `clear` in the slave's status, if the master is in packet mode,
    /// and wakes the master's readers.
    fn packet_status(&self, set: u8, clear: u8) {
        {
            let mut state = self.state.lock_or_recover();
            if !state.packet {
                return;
            }
            state.pktstatus = (state.pktstatus & !clear) | set;
        }
        if let Some(master) = self.side(Side::Master) {
            master
                .read_wait
                .wake(PollMask::IN | PollMask::PRI | PollMask::RDNORM);
        }
    }

    /// The slave's input was flushed (`n_tty_packet_mode_flush`).
    pub(super) fn input_flushed(&self, side: Side) {
        if side == Side::Slave {
            self.packet_status(TiocpktFlags::TIOCPKT_FLUSHREAD.0 as u8, 0);
        }
    }

    /// The slave's output stopped or started (`pty_stop`, `pty_start`).
    pub(super) fn flow(&self, side: Side, stopped: bool) {
        if side != Side::Slave {
            return;
        }
        let (stop, start) = (
            TiocpktFlags::TIOCPKT_STOP.0 as u8,
            TiocpktFlags::TIOCPKT_START.0 as u8,
        );
        match stopped {
            true => self.packet_status(stop, start),
            false => self.packet_status(start, stop),
        }
    }

    /// `pty_set_termios`: the slave's termios changed from `old` to `new`; a master in packet
    /// mode hears whether `^S`/`^Q` flow control is on now, and of `EXTPROC`.
    pub(super) fn termios_changed(&self, old: &Termios, new: &Termios) {
        use wasixv2_abi::{TermiosCcIndex as V, TermiosLflag};
        let flow = |termios: &Termios| {
            termios.c_iflag.0 & TermiosIflag::IXON.0 != 0
                && termios.c_cc[V::VSTOP.0 as usize] == 0o023
                && termios.c_cc[V::VSTART.0 as usize] == 0o021
        };
        let extproc = (old.c_lflag.0 | new.c_lflag.0) & TermiosLflag::EXTPROC.0 != 0;
        let (old_flow, new_flow) = (flow(old), flow(new));
        let mut set = 0;
        let mut clear = 0;
        if old_flow != new_flow {
            clear = (TiocpktFlags::TIOCPKT_DOSTOP.0 | TiocpktFlags::TIOCPKT_NOSTOP.0) as u8;
            set = match new_flow {
                true => TiocpktFlags::TIOCPKT_DOSTOP.0 as u8,
                false => TiocpktFlags::TIOCPKT_NOSTOP.0 as u8,
            };
        }
        if extproc {
            set |= TiocpktFlags::TIOCPKT_IOCTL.0 as u8;
        }
        if set != 0 {
            self.packet_status(set, clear);
        }
    }

    /// For a master read in packet mode: the slave's status, taken, if there is one.
    pub(super) fn take_status(&self) -> Option<u8> {
        let mut state = self.state.lock_or_recover();
        match (state.packet, state.pktstatus) {
            (true, status) if status != 0 => {
                state.pktstatus = 0;
                Some(status)
            }
            _ => None,
        }
    }

    /// Whether the master is in packet mode.
    pub(super) fn packet(&self) -> bool {
        self.state.lock_or_recover().packet
    }

    /// Whether a master in packet mode has a status to read (`n_tty_poll`'s `EPOLLPRI`).
    pub(super) fn has_status(&self) -> bool {
        let state = self.state.lock_or_recover();
        state.packet && state.pktstatus != 0
    }

    // ---- opening and closing (`pty_open`, `pty_close`) ----

    /// `TTY_OTHER_CLOSED` of `side`: the other side closed.
    pub(super) fn other_closed(&self, side: Side) -> bool {
        let state = self.state.lock_or_recover();
        match side {
            Side::Master => state.slave_closed,
            Side::Slave => !state.master_open,
        }
    }

    /// `pty_open` of the slave (the master opens only when made): `EIO` while it is locked or
    /// once the master closed.
    pub(super) fn open(&self, side: Side) -> Result<(), Errno> {
        if side == Side::Master {
            return Ok(());
        }
        let mut state = self.state.lock_or_recover();
        if state.locked || !state.master_open {
            return Err(Errno::EIO);
        }
        state.slave_files += 1;
        state.slave_closed = false;
        Ok(())
    }

    /// A file of `side` closes (`pty_close` for the last). The master's close hangs the slave
    /// up and takes its node away; the slave's last makes the master's reads fail once they
    /// took what remains. The index is free once neither side has a file open.
    pub(super) fn release(&self, side: Side) {
        let (last, free) = {
            let mut state = self.state.lock_or_recover();
            match side {
                Side::Master => {
                    state.master_open = false;
                    state.packet = false;
                    (true, state.slave_files == 0)
                }
                Side::Slave => {
                    state.slave_files -= 1;
                    let last = state.slave_files == 0;
                    state.slave_closed |= last;
                    (last, last && !state.master_open)
                }
            }
        };
        let hup = PollMask(PollMask::HUP.0 | PollMask::IN.0 | PollMask::OUT.0);
        match side {
            Side::Master => {
                if let Some(ptys) = self.ptys.upgrade() {
                    ptys.kill(self.index);
                }
                if let Some(slave) = self.side(Side::Slave) {
                    slave.read_wait.wake(hup);
                    slave.write_wait.wake(hup);
                    // `tty_vhangup`.
                    slave.hangup(false);
                }
            }
            Side::Slave if last => {
                if let Some(master) = self.side(Side::Master) {
                    master.read_wait.wake(hup);
                    master.write_wait.wake(hup);
                }
            }
            Side::Slave => {}
        }
        if free && let Some(ptys) = self.ptys.upgrade() {
            ptys.free(self.index);
        }
    }

    // ---- the master's `ioctl`s (`pty_unix98_ioctl`) ----

    /// `TIOCSPTLCK`.
    pub(super) fn set_lock(&self, locked: bool) {
        self.state.lock_or_recover().locked = locked;
    }

    /// `TIOCGPTLCK`.
    pub(super) fn locked(&self) -> bool {
        self.state.lock_or_recover().locked
    }

    /// `TIOCPKT` (`pty_set_pktmode`): turning packet mode on clears the slave's status.
    pub(super) fn set_packet(&self, on: bool) {
        let mut state = self.state.lock_or_recover();
        if on && !state.packet {
            state.pktstatus = 0;
        }
        state.packet = on;
    }

    /// `TIOCSIG` (`pty_signal`): `sig`, one of the signal characters', to the slave's
    /// foreground process group.
    pub(super) fn signal(&self, sig: i32) -> Result<(), Errno> {
        if ![Signal::SIGINT.0, Signal::SIGQUIT.0, Signal::SIGTSTP.0].contains(&sig) {
            return Err(Errno::EINVAL);
        }
        if let Some(slave) = self.side(Side::Slave) {
            slave.signal_pgrp(sig);
        }
        Ok(())
    }
}

impl Tty {
    /// The terminal `ioctl`s on this one act on (`tty_pair_get_tty`): a master's slave, or
    /// itself.
    pub(super) fn real_tty(self: &Arc<Self>) -> Result<Arc<Tty>, Errno> {
        match &self.driver {
            Driver::PtyMaster(pty) => pty.side(Side::Slave).ok_or(Errno::EIO),
            _ => Ok(self.clone()),
        }
    }
}

impl Kernel {
    /// `ptmx_open`: a new pair, locked, its slave's node made for the caller, and the master's
    /// file. The master never becomes a controlling terminal. `ENOSPC` when every index is
    /// taken.
    pub(crate) fn open_ptmx(&self, caller: &Caller<'_>) -> Result<TtyFile, Errno> {
        let cred = self.cred(caller);
        // `tty_reopen`: `capable(CAP_SYS_ADMIN)`.
        let admin = cred.capable_init(Cap::SysAdmin);
        let index = self.ptys.new_index()?;
        let pty = Arc::new(Pty {
            index,
            master: OnceLock::new(),
            slave: OnceLock::new(),
            to_slave: Mutex::default(),
            to_master: Mutex::default(),
            state: Mutex::new(State {
                locked: true,
                packet: false,
                pktstatus: 0,
                master_open: true,
                slave_files: 0,
                slave_closed: false,
            }),
            ptys: Arc::downgrade(&self.ptys),
        });
        let device = |major| Device {
            major,
            minor: index,
        };
        let master = Arc::new(Tty::new(
            device(PTY_MASTER_MAJOR),
            master_termios(),
            Winsize::default(),
            Driver::PtyMaster(pty.clone()),
            self.jobs.clone(),
            self.host.clone(),
        ));
        let slave = Arc::new(Tty::new(
            device(PTY_SLAVE_MAJOR),
            slave_termios(),
            Winsize::default(),
            Driver::PtySlave(pty.clone()),
            self.jobs.clone(),
            self.host.clone(),
        ));
        let _ = pty.master.set(Arc::downgrade(&master));
        let _ = pty.slave.set(Arc::downgrade(&slave));
        // `devpts_pty_new`: the node is the opener's.
        self.ptys.add(index, &slave, cred.fs_owner());
        // A new terminal is never exclusive.
        master.open(admin)
    }

    /// `ptm_open_peer` (`TIOCGPTPEER`): the slave of `pty`, whose master is `master`, opened
    /// with `flags` into a new descriptor, through its node in the pty filesystem beside the
    /// master's node, or the one the master's node is in (`devpts_mntget`). Fails like
    /// opening the slave by name.
    pub(crate) async fn open_pty_peer(
        &self,
        caller: &Caller<'_>,
        master: &OpenFile,
        pty: &Pty,
        flags: OpenFlags,
    ) -> SysResult<i32> {
        let at = master.path();
        let dir = Path {
            mnt: at.mnt.clone(),
            dentry: at.dentry.parent().ok_or(Errno::ENODEV)?,
        };
        let name = match at.mnt.sb().info.magic == DEVPTS_SUPER_MAGIC {
            true => pty.index.to_string(),
            false => format!("pts/{}", pty.index),
        };
        let cloexec = flags.contains(OpenFlags::O_CLOEXEC);
        let [fd] = self.reserve_fds(caller, cloexec)?;
        let env = self.env(caller);
        let how = OpenHow {
            flags,
            mode: 0,
            lookup: LookupFlags::follow(),
        };
        let file = self
            .vfs
            .open(&env.cx(), &dir, name.as_bytes(), &how)
            .await?;
        let theirs = self.ptys.slave(&**file.node());
        if !theirs.is_some_and(|slave| {
            pty.side(Side::Slave)
                .is_some_and(|s| Arc::ptr_eq(&s, &slave))
        }) {
            return Err(Errno::ENODEV.into());
        }
        let backing = self.attach(caller, file, flags).await?;
        Ok(fd.install(Arc::new(OpenFile::new(backing, flags))).0)
    }
}
