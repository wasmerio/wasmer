//! Signal-driven I/O, as Linux's `fs/fcntl.c`: whom an open file's I/O signals go to (its
//! owner, `fown_struct`, which `F_SETOWN` sets), the lists of open files an object signals once
//! it becomes ready (`fasync_struct`, which `O_ASYNC` puts a file on through the object's
//! `fasync`), and the signals themselves (`kill_fasync`, `send_sigio`).
//!
//! An object sends them where Linux's does, after its own locks are released, as a terminal
//! sends its foreground group `^C`. Sockets of the IP network are the exception: their
//! readiness is only seen from inside a wait-queue wakeup ([`SockAsync::watch`]), where no
//! signal may be sent (see `WaitEntry`), so their signals wait in a queue ([`Sigio`]) that the
//! timer driver empties right after. That includes `SIGURG`, which a stream socket's owner
//! gets for each new urgent mark whether or not the file is on a list (`sk_send_sigurg`).

use std::sync::{
    Arc, Mutex, OnceLock, Weak,
    atomic::{AtomicBool, Ordering},
};

use wasixv2_abi::{FOwnerEx, FOwnerType, GuestPtr, GuestPtrMut, SiCode, Signal};
use wasixv2_syscalls::{Caller, Errno, Fd, SysResult, TaskId, read_value, write_value};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitEntry, WaitRegistration};

use crate::{
    Kernel, Process,
    file::OpenFile,
    pid::PidType,
    signal::{NSIG, Signals, kernel_info},
    sync::MutexExt,
    timer::{Timer, Timers},
    tty::Jobs,
};

/// What an owner names (`fown_struct.pid_type`): one thread (`F_OWNER_TID`), a process
/// (`F_OWNER_PID`), or a process group (`F_OWNER_PGRP`, a negative `F_SETOWN`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum OwnerType {
    /// `PIDTYPE_PID`, what a file that never had an owner reports.
    #[default]
    Thread,
    Process,
    Group,
}

impl OwnerType {
    fn pid_type(self) -> PidType {
        match self {
            OwnerType::Thread => PidType::Pid,
            OwnerType::Process => PidType::Tgid,
            OwnerType::Group => PidType::Pgid,
        }
    }
}

/// Whom an open file's I/O signals go to, and which (`fown_struct`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Fown {
    /// The owner as the kernel names it (`struct pid`): a number in no namespace, which keeps
    /// naming the same task or group whatever the guest's numbers do. `None` sends nothing.
    pid: Option<u32>,
    ty: OwnerType,
    /// The real and effective uid of whoever set the owner, which the receiver's must match
    /// (`sigio_perm`).
    uid: u32,
    euid: u32,
    /// `F_SETSIG`: the signal to send with its `siginfo`, or 0 for a plain `SIGIO`.
    signum: i32,
}

impl OpenFile {
    /// The file's owner and signal, as they are now.
    pub(crate) fn fown(&self) -> Fown {
        *self.fown.lock_or_recover()
    }

    /// `__f_setown`: makes `pid` of kind `ty` the owner, as set by someone with real and
    /// effective uid `cred`; unless `force`, only if the file has no owner yet.
    pub(crate) fn set_owner(&self, pid: Option<u32>, ty: OwnerType, cred: (u32, u32), force: bool) {
        let mut fown = self.fown.lock_or_recover();
        if !force && fown.pid.is_some() {
            return;
        }
        fown.pid = pid;
        fown.ty = ty;
        if pid.is_some() {
            (fown.uid, fown.euid) = cred;
        }
    }

    /// What removing a lease does to its file (`lease_modify` with `F_UNLCK`): no owner
    /// (`f_delown`) and no `F_SETSIG` signal.
    pub(crate) fn delown(&self) {
        let mut fown = self.fown.lock_or_recover();
        fown.pid = None;
        fown.ty = OwnerType::Process;
        fown.signum = 0;
    }
}

/// What an object's `fasync` gets: the open file, and with `on`, what puts it on a list.
pub struct Fasync<'a> {
    pub(crate) file: &'a OpenFile,
    /// `None` takes the file off the object's lists.
    pub(crate) on: Option<FasyncOn<'a>>,
}

/// What a file goes on an object's list with.
pub struct FasyncOn<'a> {
    pub(crate) file: &'a Arc<OpenFile>,
    /// The descriptor the signals name (`si_fd`): the one `O_ASYNC` was set through.
    pub(crate) fd: Fd,
    pub(crate) sigio: &'a Arc<Sigio>,
    /// The caller, whom an object that picks an owner itself names (`tty_fasync`).
    pub(crate) task: TaskId,
    /// The caller's real and effective uid.
    pub(crate) cred: (u32, u32),
}

/// One open file on an object's list (`fasync_struct`).
struct Entry {
    file: Weak<OpenFile>,
    fd: Fd,
    sigio: Arc<Sigio>,
}

/// The open files an object sends I/O signals to (a `fasync_struct` list).
#[derive(Default)]
pub(crate) struct FasyncList(Mutex<Vec<Entry>>);

impl FasyncList {
    /// `fasync_helper`: puts the file on the list, or with the descriptor it now names if it
    /// is there already, or takes it off; the file's `O_ASYNC` follows whether it is on a list.
    /// Returns whether the list changed.
    pub(crate) fn helper(&self, fasync: &Fasync<'_>) -> bool {
        let mut list = self.0.lock_or_recover();
        let at = list
            .iter()
            .position(|entry| std::ptr::eq(entry.file.as_ptr(), fasync.file));
        match (&fasync.on, at) {
            (Some(on), Some(at)) => {
                list[at].fd = on.fd;
                false
            }
            (Some(on), None) => {
                list.push(Entry {
                    file: Arc::downgrade(on.file),
                    fd: on.fd,
                    sigio: on.sigio.clone(),
                });
                fasync.file.set_fasync(true);
                true
            }
            (None, Some(at)) => {
                list.remove(at);
                fasync.file.set_fasync(false);
                true
            }
            (None, None) => false,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.lock_or_recover().is_empty()
    }

    /// Takes every file off the list, as a terminal's hangup does with each of its files
    /// (`__tty_fasync(-1, filp, 0)`).
    pub(crate) fn clear(&self) {
        let list = std::mem::take(&mut *self.0.lock_or_recover());
        for entry in list {
            if let Some(file) = entry.file.upgrade() {
                file.set_fasync(false);
            }
        }
    }

    /// `kill_fasync(list, SIGIO, band)`: each file's owner hears that the object became ready
    /// for `band` (`POLL_IN`, `POLL_OUT`, ...). Called with none of the object's locks held.
    pub(crate) fn kill(&self, band: SiCode) {
        for (file, fd, sigio) in self.files() {
            sigio.send(&file, fd, band);
        }
    }

    /// `kill_fasync(list, SIGURG, POLL_PRI)`: only the files with an `F_SETSIG` signal, since
    /// `SIGURG` itself went to the owner already (`send_sigurg`).
    fn kill_urg(&self) {
        for (file, fd, sigio) in self.files() {
            if file.fown().signum != 0 {
                sigio.send(&file, fd, SiCode::POLL_PRI);
            }
        }
    }

    /// [`FasyncList::kill`] from where no signal may be sent: the signals go once the timer
    /// driver runs.
    /// The files are not taken from the list's references: the last one going here would
    /// release the file inside a wakeup.
    fn kill_later(&self, band: SiCode) {
        let list: Vec<_> = {
            let list = self.0.lock_or_recover();
            list.iter()
                .map(|e| (e.file.clone(), e.fd, e.sigio.clone()))
                .collect()
        };
        for (file, fd, sigio) in list {
            sigio.queue(Queued::Io(file, fd, band));
        }
    }

    /// The files still open, with what they were put on the list with.
    fn files(&self) -> Vec<(Arc<OpenFile>, Fd, Arc<Sigio>)> {
        let list = self.0.lock_or_recover();
        list.iter()
            .filter_map(|e| Some((e.file.upgrade()?, e.fd, e.sigio.clone())))
            .collect()
    }
}

/// `band_table`: the poll events a `POLL_*` code stands for, as `si_band` reports them.
fn band_events(band: SiCode) -> PollMask {
    match band {
        SiCode::POLL_IN => PollMask::IN | PollMask::RDNORM,
        SiCode::POLL_OUT => PollMask::OUT | PollMask::WRNORM | PollMask::WRBAND,
        SiCode::POLL_MSG => PollMask::IN | PollMask::RDNORM | PollMask::MSG,
        SiCode::POLL_ERR => PollMask::ERR,
        SiCode::POLL_PRI => PollMask::PRI | PollMask::RDBAND,
        _ => PollMask::HUP | PollMask::ERR,
    }
}

/// `sig_specific_sicodes`: the signals whose positive `si_code`s mean something of their own,
/// so an I/O signal among them says `SI_SIGIO` instead of a `POLL_*` code.
fn sig_specific_sicodes(sig: i32) -> bool {
    matches!(
        Signal(sig),
        Signal::SIGILL
            | Signal::SIGFPE
            | Signal::SIGSEGV
            | Signal::SIGBUS
            | Signal::SIGTRAP
            | Signal::SIGCHLD
            | Signal::SIGPOLL
            | Signal::SIGSYS
    )
}

/// A signal waiting for the timer driver.
enum Queued {
    /// An I/O signal of a file for a band, naming a descriptor.
    Io(Weak<OpenFile>, Fd, SiCode),
    /// `sk_send_sigurg` of a socket.
    Urg(Weak<SockAsync>),
}

/// Sends I/O signals to owners, now ([`Sigio::send`]) or once the timer driver runs
/// ([`Sigio::queue`]). One per machine.
pub(crate) struct Sigio {
    jobs: Jobs,
    /// The signals waiting for the driver, in order.
    queued: Mutex<Vec<Queued>>,
    /// Fires at once when armed, to send what is queued.
    timer: Timer,
}

impl Sigio {
    pub(crate) fn new(jobs: Jobs, timers: &Timers) -> Arc<Self> {
        Arc::new_cyclic(|me: &Weak<Sigio>| {
            let me = me.clone();
            let timer = timers.timer(Box::new(move || {
                if let Some(sigio) = me.upgrade() {
                    sigio.run();
                }
                None
            }));
            Self {
                jobs,
                queued: Mutex::default(),
                timer,
            }
        })
    }

    /// Queues a signal, and arms the timer if nothing was queued.
    fn queue(&self, signal: Queued) {
        let first = {
            let mut queued = self.queued.lock_or_recover();
            queued.push(signal);
            queued.len() == 1
        };
        if first {
            self.timer.start(std::time::Duration::ZERO);
        }
    }

    /// Sends what is queued, from the timer driver with nothing locked.
    fn run(&self) {
        let queued = std::mem::take(&mut *self.queued.lock_or_recover());
        for signal in queued {
            match signal {
                Queued::Io(file, fd, band) => {
                    if let Some(file) = file.upgrade() {
                        self.send(&file, fd, band);
                    }
                }
                Queued::Urg(sock) => {
                    if let Some(sock) = sock.upgrade() {
                        sock.send_urg(self);
                    }
                }
            }
        }
    }

    /// `send_sigurg`: `SIGURG` to the owner of `file`, which has urgent data, as `send_sigio`
    /// picks the receivers but without `F_SETSIG`'s signal. Returns whether the file has an
    /// owner.
    fn send_sigurg(&self, file: &OpenFile) -> bool {
        let fown = file.fown();
        let Some(pid) = fown.pid else {
            return false;
        };
        let info = || kernel_info(Signal::SIGURG.0);
        match fown.ty {
            OwnerType::Thread | OwnerType::Process => {
                if let Some((process, signals)) = self.jobs.live_task(pid)
                    && sigio_perm(&process, &fown)
                {
                    let task = (fown.ty == OwnerType::Thread).then_some(TaskId(pid));
                    let _ = signals.group().send_from(task, info(), false);
                }
            }
            OwnerType::Group => {
                for (process, signals) in self.jobs.pgrp_processes(pid) {
                    if sigio_perm(&process, &fown) {
                        let _ = signals.send_from(None, info(), false);
                    }
                }
            }
        }
        true
    }

    /// `send_sigio`: `file`'s I/O signal for `band` to its owner, naming descriptor `fd`:
    /// to the one task of a thread owner, to the process of a process owner, to every process
    /// of a group owner.
    pub(crate) fn send(&self, file: &OpenFile, fd: Fd, band: SiCode) {
        let fown = file.fown();
        let Some(pid) = fown.pid else {
            return;
        };
        match fown.ty {
            OwnerType::Thread | OwnerType::Process => {
                let Some((process, signals)) = self.jobs.live_task(pid) else {
                    return;
                };
                let task = (fown.ty == OwnerType::Thread).then_some(TaskId(pid));
                send_to_task(&fown, &process, signals.group(), task, fd, band);
            }
            OwnerType::Group => {
                for (process, signals) in self.jobs.pgrp_processes(pid) {
                    send_to_task(&fown, &process, &signals, None, fd, band);
                }
            }
        }
    }
}

/// `sigio_perm`: whoever set the owner is root, or its real or effective uid is the
/// receiver's real or saved one.
fn sigio_perm(process: &Process, fown: &Fown) -> bool {
    let cred = process.cred.lock_or_recover();
    let (uid, suid) = (cred.user.real, cred.user.saved);
    fown.euid == 0
        || [fown.uid, fown.euid]
            .iter()
            .any(|&id| id == uid || id == suid)
}

/// `send_sigio_to_task`: the `F_SETSIG` signal with the descriptor and the band in its
/// `siginfo`, or plain SIGIO from the kernel without one, or when the realtime signal finds no
/// room in the receiver's queue. To `task` alone, or to the whole process.
fn send_to_task(
    fown: &Fown,
    process: &Process,
    signals: &Signals,
    task: Option<TaskId>,
    fd: Fd,
    band: SiCode,
) {
    if !sigio_perm(process, fown) {
        return;
    }
    if fown.signum != 0 {
        let mut info = kernel_info(fown.signum);
        // Linux gives other signals the `POLL_*` codes too, unless theirs mean something else.
        info.si_code = match fown.signum != Signal::SIGPOLL.0 && sig_specific_sicodes(fown.signum) {
            true => SiCode::SI_SIGIO.0,
            false => band.0,
        };
        // `si_band` is a long, 32 bits here, and `si_fd` follows it.
        let events = band_events(band).bits() as i32;
        info.sifields[0..4].copy_from_slice(&events.to_le_bytes());
        info.sifields[4..8].copy_from_slice(&fd.0.to_le_bytes());
        if signals.send_from(task, info, false).is_ok() {
            return;
        }
    }
    let _ = signals.send_from(task, kernel_info(Signal::SIGIO.0), false);
}

// ---- sockets ----

/// `sk_wake_async`'s `how`: why a socket signals.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SockWake {
    /// `SOCK_WAKE_IO`: always.
    Io,
    /// `SOCK_WAKE_WAITD`: data or an end arrived.
    WaitData,
    /// `SOCK_WAKE_SPACE`: room to send appeared, which matters only once a send found none.
    Space,
}

/// A socket's part in signal-driven I/O (`socket_wq`'s `fasync_list` and its flags), for
/// every family. Its files are on its list through `sock_fasync`.
#[derive(Default)]
pub(crate) struct SockAsync {
    list: FasyncList,
    /// `SOCKWQ_ASYNC_NOSPACE`: a send found no room, so room appearing is a `POLL_OUT`.
    nospace: AtomicBool,
    /// For a stream socket of the IP network, its open file (`sock->file`), whose owner
    /// hears of urgent data, and what sends that.
    urgent: OnceLock<(Weak<OpenFile>, Arc<Sigio>)>,
    /// The entry on the queue of a socket whose wakeups are watched ([`SockAsync::watch`]),
    /// while its list has files or it may have urgent data.
    watching: Mutex<Option<WaitRegistration>>,
}

impl SockAsync {
    /// `sock_fasync`.
    pub(crate) fn fasync(&self, fasync: &Fasync<'_>) -> Result<(), Errno> {
        self.list.helper(fasync);
        Ok(())
    }

    /// `sock_wake_async`: the owners of the socket's files hear of `band`, unless `how` says
    /// it does not matter now.
    pub(crate) fn wake(&self, how: SockWake, band: SiCode) {
        if self.list.is_empty() || !self.wanted(how) {
            return;
        }
        self.list.kill(band);
    }

    fn wanted(&self, how: SockWake) -> bool {
        match how {
            SockWake::Space => self.nospace.swap(false, Ordering::Relaxed),
            SockWake::Io | SockWake::WaitData => true,
        }
    }

    /// `sk_set_bit(SOCKWQ_ASYNC_NOSPACE)`: a send found no room.
    pub(crate) fn set_nospace(&self) {
        self.nospace.store(true, Ordering::Relaxed);
    }

    /// [`SockAsync::fasync`] for a socket whose readiness the kernel only sees in its
    /// wakeups (one of the IP network's): while files are on the list, an entry on the queues
    /// `socket` polls on turns each wakeup into what Linux's socket callbacks would signal.
    pub(crate) fn watch(
        self: &Arc<Self>,
        socket: &dyn Pollable,
        fasync: &Fasync<'_>,
    ) -> Result<(), Errno> {
        self.fasync(fasync)?;
        self.rewatch(socket);
        Ok(())
    }

    /// Makes `file`, the open file of `socket`, a stream socket of the IP network, hear of
    /// urgent data for as long as it lives (`sk_send_sigurg` through `sk->sk_socket->file`).
    pub(crate) fn watch_urgent(
        self: &Arc<Self>,
        socket: &dyn Pollable,
        file: &Arc<OpenFile>,
        sigio: &Arc<Sigio>,
    ) {
        let _ = self.urgent.set((Arc::downgrade(file), sigio.clone()));
        self.rewatch(socket);
    }

    /// Puts the entry on the queues `socket` polls on while it has something to signal, and
    /// takes it off otherwise.
    fn rewatch(self: &Arc<Self>, socket: &dyn Pollable) {
        let mut watching = self.watching.lock_or_recover();
        let wanted = !self.list.is_empty() || self.urgent.get().is_some();
        match (wanted, watching.is_some()) {
            (true, false) => {
                let entry: Arc<dyn WaitEntry> = Arc::new(Watch(Arc::downgrade(self)));
                let mut queue_proc = |queue: &wasixv2_wait::WaitQueue, _: PollMask| {
                    *watching = Some(queue.add(entry.clone()));
                };
                socket.poll(Some(&mut PollTable::new(
                    PollMask::empty(),
                    &mut queue_proc,
                )));
            }
            (false, true) => *watching = None,
            _ => {}
        }
    }

    /// `sk_send_sigurg`, from the timer driver: `SIGURG` to the file's owner, then, if it has
    /// one, `sk_wake_async(SOCK_WAKE_URG, POLL_PRI)`.
    fn send_urg(&self, sigio: &Sigio) {
        let Some(file) = self.urgent.get().and_then(|(file, _)| file.upgrade()) else {
            return;
        };
        if sigio.send_sigurg(&file) {
            self.list.kill_urg();
        }
    }
}

/// The entry [`SockAsync::watch`] puts on a socket's queue.
struct Watch(Weak<SockAsync>);

impl WaitEntry for Watch {
    /// The socket callbacks a wakeup stands for: a new urgent mark wakes with `PRI`
    /// (`sk_send_sigurg`, before the data it comes with); `sk_error_report` (`POLL_ERR`),
    /// `sk_data_ready` (`POLL_IN`) and `sk_write_space` (`POLL_OUT`) wake with their events;
    /// `sk_state_change` wakes with none and signals nothing.
    fn wake(&self, events: PollMask) -> bool {
        let Some(sock) = self.0.upgrade() else {
            return false;
        };
        if events.contains(PollMask::PRI)
            && let Some((_, sigio)) = sock.urgent.get()
        {
            sigio.queue(Queued::Urg(self.0.clone()));
        }
        if sock.list.is_empty() {
            return false;
        }
        let (how, band) = if events.intersects(PollMask::ERR) {
            (SockWake::Io, SiCode::POLL_ERR)
        } else if events.intersects(PollMask::IN) {
            (SockWake::WaitData, SiCode::POLL_IN)
        } else if events.intersects(PollMask::OUT) {
            (SockWake::Space, SiCode::POLL_OUT)
        } else {
            return false;
        };
        if sock.wanted(how) {
            sock.list.kill_later(band);
        }
        false
    }
}

impl Kernel {
    /// The object's `fasync` for `file`, open as `fd`: puts it on the object's lists (`on`) or
    /// takes it off. `None` when the object has none.
    pub(crate) fn fasync(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        file: &Arc<OpenFile>,
        on: bool,
    ) -> Option<Result<(), Errno>> {
        let object = file.object()?;
        let cred = self.cred(caller);
        let on = on.then(|| FasyncOn {
            file,
            fd,
            sigio: &self.sigio,
            task: caller.task(),
            cred: (cred.user.real, cred.user.effective),
        });
        object.fasync(&Fasync { file, on })
    }

    /// `f_setown`: `who` names a process, or negated a process group, in the caller's pid
    /// namespace; 0 clears the owner.
    pub(crate) fn f_setown(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        who: i32,
    ) -> SysResult<i32> {
        let (ty, nr) = match who {
            i32::MIN => return Err(Errno::EINVAL.into()),
            who if who < 0 => (OwnerType::Group, -who),
            _ => (OwnerType::Process, who),
        };
        let pid = match nr {
            0 => None,
            _ => Some(self.find_vpid(caller, nr).ok_or(Errno::ESRCH)?),
        };
        file.set_owner(pid, ty, self.uids(caller), true);
        Ok(0)
    }

    /// `f_getown`: the owner as the caller's pid namespace numbers it, negated for a process
    /// group; 0 when there is none, it is gone or it is outside the namespace.
    pub(crate) fn f_getown(&self, caller: &Caller<'_>, file: &OpenFile) -> i32 {
        let (nr, ty) = self.owner_nr(caller, &file.fown());
        match ty {
            OwnerType::Group => -nr,
            _ => nr,
        }
    }

    /// `f_setown_ex`.
    pub(crate) async fn f_setown_ex(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        arg: u32,
    ) -> SysResult<i32> {
        let owner: FOwnerEx = read_value(caller.memory(), GuestPtr::new(arg)).await?;
        let ty = match owner.r#type {
            FOwnerType::F_OWNER_TID => OwnerType::Thread,
            FOwnerType::F_OWNER_PID => OwnerType::Process,
            FOwnerType::F_OWNER_PGRP => OwnerType::Group,
            _ => return Err(Errno::EINVAL.into()),
        };
        let pid = self.find_vpid(caller, owner.pid);
        if owner.pid != 0 && pid.is_none() {
            return Err(Errno::ESRCH.into());
        }
        file.set_owner(pid, ty, self.uids(caller), true);
        Ok(0)
    }

    /// `f_getown_ex`.
    pub(crate) async fn f_getown_ex(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        arg: u32,
    ) -> SysResult<i32> {
        let (pid, ty) = self.owner_nr(caller, &file.fown());
        let ty = match ty {
            OwnerType::Thread => FOwnerType::F_OWNER_TID,
            OwnerType::Process => FOwnerType::F_OWNER_PID,
            OwnerType::Group => FOwnerType::F_OWNER_PGRP,
        };
        let owner = FOwnerEx { r#type: ty, pid };
        write_value(caller.memory(), GuestPtrMut::new(arg), &owner).await?;
        Ok(0)
    }

    /// `f_getowner_uids`: the real and effective uid of whoever set the owner.
    pub(crate) async fn f_getowner_uids(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        arg: u32,
    ) -> SysResult<i32> {
        let fown = file.fown();
        write_value(
            caller.memory(),
            GuestPtrMut::new(arg),
            &[fown.uid, fown.euid],
        )
        .await?;
        Ok(0)
    }

    /// `F_SETSIG` (`f_owner_sig`): any valid signal, or 0 for plain SIGIO.
    pub(crate) fn f_setsig(&self, file: &OpenFile, sig: u32) -> SysResult<i32> {
        let sig = i32::try_from(sig)
            .ok()
            .filter(|&sig| sig <= NSIG)
            .ok_or(Errno::EINVAL)?;
        file.fown.lock_or_recover().signum = sig;
        Ok(0)
    }

    /// `F_GETSIG`.
    pub(crate) fn f_getsig(&self, file: &OpenFile) -> i32 {
        file.fown().signum
    }

    /// The owner's number in the caller's pid namespace, 0 unless something of its kind is
    /// still there (`pid_task`, `pid_vnr`), and its kind.
    fn owner_nr(&self, caller: &Caller<'_>, fown: &Fown) -> (i32, OwnerType) {
        let nr = fown
            .pid
            .filter(|&pid| self.pids.attached(pid, fown.ty.pid_type()))
            .map_or(0, |pid| self.vnr(caller, pid));
        (nr, fown.ty)
    }

    /// The caller's real and effective uid, which an owner it sets records.
    fn uids(&self, caller: &Caller<'_>) -> (u32, u32) {
        let cred = self.cred(caller);
        (cred.user.real, cred.user.effective)
    }
}
