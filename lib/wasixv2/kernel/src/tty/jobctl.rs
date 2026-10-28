//! Job control's side of terminals (`tty_jobctrl.c`): the controlling terminal a session
//! acquires and loses, its foreground process group, and what a process outside that group
//! may do with it.

use std::sync::{Arc, Mutex, Weak};

use wasixv2_abi::Signal;
use wasixv2_syscalls::{Errno, SysErr, SysResult, TaskId};

use super::{Ctrl, Tty};
use crate::{
    Kernel, Process, TaskTable,
    ident::Ident,
    process::Tree,
    signal::{Signals, TaskSignals},
    sync::MutexExt,
};

/// What terminals need of the task table and the process tree: process groups and sessions,
/// and signals to them. Weak, as terminals belong to processes in the tree.
#[derive(Clone)]
pub(crate) struct Jobs {
    tasks: Weak<TaskTable>,
    tree: Weak<Mutex<Tree>>,
}

impl Jobs {
    pub(crate) fn new(tasks: &Arc<TaskTable>, tree: &Arc<Mutex<Tree>>) -> Self {
        Self {
            tasks: Arc::downgrade(tasks),
            tree: Arc::downgrade(tree),
        }
    }

    /// The process `task` belongs to.
    pub(crate) fn process(&self, task: TaskId) -> Option<Arc<Process>> {
        let tasks = self.tasks.upgrade()?;
        let state = tasks.read_or_recover();
        state.tasks.get(&task).map(|task| task.process.clone())
    }

    fn task_signals(&self, task: TaskId) -> Option<Arc<TaskSignals>> {
        let tasks = self.tasks.upgrade()?;
        let state = tasks.read_or_recover();
        state.tasks.get(&task).map(|task| task.signals.clone())
    }

    /// The signals of the thread group `pid` leads.
    pub(crate) fn group_signals(&self, pid: u32) -> Option<Arc<Signals>> {
        let tasks = self.tasks.upgrade()?;
        let state = tasks.read_or_recover();
        let task = state.tasks.get(&TaskId(pid))?;
        Some(task.signals.group().clone())
    }

    /// The thread groups in process group `pgid` that have not exited.
    pub(crate) fn pgrp_groups(&self, pgid: u32) -> Vec<(u32, Arc<Signals>)> {
        let members = self.members(|ident| ident.pgid == pgid);
        members
            .into_iter()
            .filter_map(|process| {
                let pid = process.pid.0;
                Some((pid, self.group_signals(pid)?))
            })
            .collect()
    }

    /// The live processes whose identity `select` picks.
    fn members(&self, select: impl Fn(&Ident) -> bool) -> Vec<Arc<Process>> {
        let Some(tree) = self.tree.upgrade() else {
            return Vec::new();
        };
        let tree = tree.lock_or_recover();
        tree.nodes
            .values()
            .filter(|node| !node.is_zombie() && select(&node.process.ident.lock_or_recover()))
            .map(|node| node.process.clone())
            .collect()
    }

    /// `kill_pgrp(pgrp, sig, 1)`: `sig` from the kernel (`SI_KERNEL`) to every process of the
    /// group. Returns whether some process has it pending now; `false` when each one ignores
    /// it, so it was discarded where it was generated.
    pub(crate) fn kill_pgrp(&self, pgid: u32, sig: i32) -> bool {
        let mut pending = false;
        for (_, signals) in self.pgrp_groups(pgid) {
            pending |= signals.generate_kernel(sig);
        }
        pending
    }

    /// `is_pgrp_orphaned`: no process of the group has a parent in another group of the same
    /// session, which could bring it back from a stop.
    pub(crate) fn is_pgrp_orphaned(&self, pgid: u32) -> bool {
        self.tree
            .upgrade()
            .is_none_or(|tree| tree.lock_or_recover().will_become_orphaned_pgrp(pgid, None))
    }

    /// `session_of_pgrp` for `TIOCSPGRP`: the session of the group `nr`, or of the process
    /// `nr` if no group has that id; `None` if neither exists (`find_vpid`).
    fn session_of_pgrp(&self, nr: u32) -> Option<u32> {
        let in_group = self.members(|ident| ident.pgid == nr);
        let process = match in_group.first() {
            Some(process) => process.clone(),
            None => self
                .members(|_| true)
                .into_iter()
                .find(|process| process.pid.0 == nr)?,
        };
        let sid = process.ident.lock_or_recover().sid;
        Some(sid)
    }

    /// `session_clear_tty`: no process of the session `sid` has a controlling terminal any more.
    fn session_clear_tty(&self, sid: u32) {
        for process in self.members(|ident| ident.sid == sid) {
            process.ident.lock_or_recover().tty = None;
        }
    }
}

impl Tty {
    /// Whether `ident` has this terminal as its controlling terminal.
    fn controls(&self, ident: &Ident) -> bool {
        ident
            .tty
            .as_ref()
            .is_some_and(|tty| std::ptr::eq(Arc::as_ptr(tty), self))
    }

    /// `__tty_check_change`: a process of a background group that changes its controlling
    /// terminal, or reads it (`sig` SIGTTIN), or writes it under `TOSTOP` (SIGTTOU), stops
    /// its group with `sig` and tries again once continued. Unless the signal is blocked or
    /// ignored: then a read fails with `EIO` and a change goes ahead; or unless the group is
    /// orphaned and nothing could continue it: `EIO`.
    pub(crate) fn check_change(&self, task: TaskId, sig: Signal) -> SysResult<()> {
        let Some(process) = self.jobs.process(task) else {
            return Ok(());
        };
        let pgid = {
            let ident = process.ident.lock_or_recover();
            if !self.controls(&ident) {
                return Ok(());
            }
            ident.pgid
        };
        if self.pgrp().is_none_or(|pgrp| pgrp == pgid) {
            return Ok(());
        }
        let ignored = self
            .jobs
            .task_signals(task)
            .is_some_and(|signals| signals.ignores_or_blocks(sig.0));
        if ignored {
            return match sig == Signal::SIGTTIN {
                true => Err(Errno::EIO.into()),
                false => Ok(()),
            };
        }
        if self.jobs.is_pgrp_orphaned(pgid) {
            return Err(Errno::EIO.into());
        }
        self.jobs.kill_pgrp(pgid, sig.0);
        Err(SysErr::Restart)
    }

    /// `__proc_set_tty`: the terminal becomes the controlling terminal of the session `ident`
    /// leads, with its group in the foreground.
    fn set_ctty(self: &Arc<Self>, ident: &mut Ident) {
        *self.ctrl.lock_or_recover() = Ctrl {
            session: Some(ident.sid),
            pgrp: Some(ident.pgid),
        };
        ident.tty = Some(self.clone());
    }

    /// `tty_open_proc_set_tty`: a session leader without a controlling terminal that opens one
    /// no session has, for reading, acquires it.
    pub(crate) fn open_set_ctty(self: &Arc<Self>, process: &Process, readable: bool) {
        let mut ident = process.ident.lock_or_recover();
        let free = self.ctrl.lock_or_recover().session.is_none();
        if ident.sid == process.pid.0 && ident.tty.is_none() && free && readable {
            self.set_ctty(&mut ident);
        }
    }

    /// `tiocsctty`: the caller, a session leader without a controlling terminal, acquires this
    /// one. Taking it from the session it controls needs `steal` (argument 1) and `admin`
    /// (`CAP_SYS_ADMIN`), and so does a file not open for reading.
    pub(crate) fn set_ctty_ioctl(
        self: &Arc<Self>,
        process: &Process,
        readable: bool,
        steal: bool,
        admin: bool,
    ) -> Result<(), Errno> {
        let (leader, sid, has_tty) = {
            let ident = process.ident.lock_or_recover();
            (ident.sid == process.pid.0, ident.sid, ident.tty.is_some())
        };
        let session = self.ctrl.lock_or_recover().session;
        if leader && session == Some(sid) {
            return Ok(());
        }
        if !leader || has_tty {
            return Err(Errno::EPERM);
        }
        if let Some(other) = session {
            if !(steal && admin) {
                return Err(Errno::EPERM);
            }
            self.jobs.session_clear_tty(other);
        }
        if !readable && !admin {
            return Err(Errno::EPERM);
        }
        self.set_ctty(&mut process.ident.lock_or_recover());
        Ok(())
    }

    /// `tiocspgrp`: the caller's session moves its group `pgrp` to the foreground; `None` for a
    /// number naming nothing (`ESRCH`).
    pub(crate) fn set_pgrp(&self, process: &Process, pgrp: Option<u32>) -> Result<(), Errno> {
        let sid = {
            let ident = process.ident.lock_or_recover();
            let session = self.ctrl.lock_or_recover().session;
            if !self.controls(&ident) || session != Some(ident.sid) {
                return Err(Errno::ENOTTY);
            }
            ident.sid
        };
        let pgrp = pgrp.ok_or(Errno::ESRCH)?;
        match self.jobs.session_of_pgrp(pgrp) {
            None => Err(Errno::ESRCH),
            Some(other) if other != sid => Err(Errno::EPERM),
            Some(_) => {
                self.ctrl.lock_or_recover().pgrp = Some(pgrp);
                Ok(())
            }
        }
    }

    /// The session this terminal controls, for `TIOCGSID`.
    pub(crate) fn session(&self) -> Option<u32> {
        self.ctrl.lock_or_recover().session
    }

    /// Whether this is the controlling terminal of `process`.
    pub(crate) fn is_ctty_of(&self, process: &Process) -> bool {
        self.controls(&process.ident.lock_or_recover())
    }

    /// `tty_signal_session_leader`: every process of the session loses the terminal, and its
    /// leader gets SIGHUP and SIGCONT. Returns the foreground group.
    pub(super) fn signal_session_leader(&self) -> Option<u32> {
        let Ctrl { session, pgrp } = *self.ctrl.lock_or_recover();
        let Some(sid) = session else {
            return pgrp;
        };
        for process in self.jobs.members(|ident| ident.sid == sid) {
            let leader = {
                let mut ident = process.ident.lock_or_recover();
                if self.controls(&ident) {
                    ident.tty = None;
                }
                ident.sid == process.pid.0
            };
            if leader && let Some(signals) = self.jobs.group_signals(process.pid.0) {
                signals.send_kernel(Signal::SIGHUP.0);
                signals.send_kernel(Signal::SIGCONT.0);
            }
        }
        pgrp
    }
}

impl Kernel {
    /// `disassociate_ctty`: a session leader gives up its controlling terminal, as it exits
    /// (`on_exit`) or with `TIOCNOTTY`. On exit the terminal is hung up, unless it is a pty
    /// slave, whose foreground group only gets SIGHUP; otherwise the foreground group gets
    /// SIGHUP and SIGCONT. Either way no process of the session has the terminal any more.
    pub(crate) fn disassociate_ctty(&self, process: &Process, on_exit: bool) {
        let (sid, tty) = {
            let ident = process.ident.lock_or_recover();
            if ident.sid != process.pid.0 {
                return;
            }
            (ident.sid, ident.tty.clone())
        };
        let Some(tty) = tty else {
            return;
        };
        if on_exit && tty.driver.hangs_up_on_exit() {
            tty.hangup(true);
        } else if let Some(pgrp) = tty.pgrp() {
            self.jobs.kill_pgrp(pgrp, Signal::SIGHUP.0);
            if !on_exit {
                self.jobs.kill_pgrp(pgrp, Signal::SIGCONT.0);
            }
        }
        *tty.ctrl.lock_or_recover() = Ctrl::default();
        self.jobs.session_clear_tty(sid);
    }

    /// `no_tty` for `TIOCNOTTY`: the caller gives up its controlling terminal, and a session
    /// leader takes it from its whole session.
    pub(crate) fn no_tty(&self, process: &Process) {
        self.disassociate_ctty(process, false);
        process.ident.lock_or_recover().tty = None;
    }
}
