//! Pids and pid namespaces (`kernel/pid.c`, `kernel/pid_namespace.c`).
//!
//! A task's [`TaskId`] is the kernel's own name for it, Linux's `struct pid`: never reused, and
//! never shown to a guest. What a guest sees is the task's number in a pid namespace: a task has
//! one in the namespace it starts in and in each ancestor of it ([`Upid`]), allocated cyclically
//! up to the namespace's `pid_max` and reused once free (`alloc_pid`). The numbers stay while
//! something is attached to the id (`__change_pid`): the task itself, the thread group it
//! leads, and every process whose process group or session it names. Every pid that crosses
//! the syscall or `/proc` boundary is translated, out with [`Pids::nr`] (`pid_nr_ns`) and in
//! with [`PidSpace::find`] (`find_pid_ns`), in the pid namespace of whoever asks.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, RwLock},
};

use wasixv2_abi::Pid;
use wasixv2_syscalls::{Caller, Errno, TaskId};

use crate::{
    Kernel,
    ns::PidNs,
    sync::{MutexExt, RwLockExt},
};

/// `MAX_PID_NS_LEVEL`: how deep pid namespaces nest below the initial one.
pub(crate) const MAX_PID_NS_LEVEL: usize = 32;
/// `RESERVED_PIDS`: where allocation starts over once it wrapped at `pid_max`.
const RESERVED_PIDS: u32 = 300;
/// `PID_MAX_DEFAULT`: the initial pid namespace's `pid_max`.
pub(crate) const PID_MAX_DEFAULT: u32 = 0x8000;
/// `PID_MAX_LIMIT` on 64-bit Linux: the most `pid_max` can be, and that of a namespace made
/// after boot.
pub(crate) const PID_MAX_LIMIT: u32 = 4 * 1024 * 1024;
/// `pid_max_min`: the least `pid_max` can be set to.
pub(crate) const PID_MAX_MIN: u32 = RESERVED_PIDS + 1;

/// What a pid namespace holds of its own: its place in the hierarchy and its numbers.
pub(crate) struct PidSpace {
    parent: Option<Arc<PidNs>>,
    /// How deep it is: 0 for the initial namespace.
    level: usize,
    state: Mutex<SpaceState>,
}

struct SpaceState {
    /// The numbers in use, to the id that has each (`idr`).
    used: BTreeMap<u32, u32>,
    /// Where the next search for a free number starts (`idr_get_cursor`): one past the last
    /// number handed out.
    cursor: u32,
    pid_max: u32,
    /// New numbers are handed out (`PIDNS_ADDING`): until the namespace's init exits.
    adding: bool,
    /// The process that got number 1, which adopts the namespace's orphans
    /// (`child_reaper`).
    reaper: Option<u32>,
    /// The reaper exited and waits for every other number to go before it is a zombie
    /// (`zap_pid_ns_processes`).
    zapping: bool,
}

impl PidSpace {
    /// The initial pid namespace's.
    pub(crate) fn initial() -> Self {
        Self::new(None, 0, PID_MAX_DEFAULT)
    }

    /// A namespace below `parent` (`create_pid_namespace`): `ENOSPC` past
    /// [`MAX_PID_NS_LEVEL`]. Its init must be the first process it gets.
    pub(crate) fn child_of(parent: &Arc<PidNs>) -> Result<Self, Errno> {
        let level = parent.level + 1;
        if level > MAX_PID_NS_LEVEL {
            return Err(Errno::ENOSPC);
        }
        Ok(Self::new(Some(parent.clone()), level, PID_MAX_LIMIT))
    }

    fn new(parent: Option<Arc<PidNs>>, level: usize, pid_max: u32) -> Self {
        Self {
            parent,
            level,
            state: Mutex::new(SpaceState {
                used: BTreeMap::new(),
                cursor: 0,
                pid_max,
                adding: true,
                reaper: None,
                zapping: false,
            }),
        }
    }

    pub(crate) fn parent(&self) -> Option<&Arc<PidNs>> {
        self.parent.as_ref()
    }

    pub(crate) fn level(&self) -> usize {
        self.level
    }

    /// `find_pid_ns`: the id that has `nr` here.
    pub(crate) fn find(&self, nr: Pid) -> Option<u32> {
        let nr = u32::try_from(nr).ok()?;
        self.state.lock_or_recover().used.get(&nr).copied()
    }

    /// The ids with a number here, lowest number first, with their numbers.
    pub(crate) fn members(&self) -> Vec<(u32, u32)> {
        let state = self.state.lock_or_recover();
        state.used.iter().map(|(&nr, &id)| (nr, id)).collect()
    }

    /// The process that adopts the namespace's orphans (`child_reaper`), if it has one yet.
    pub(crate) fn reaper(&self) -> Option<u32> {
        self.state.lock_or_recover().reaper
    }

    /// `ns_last_pid`: the last number handed out (`idr_get_cursor - 1`).
    pub(crate) fn last_pid(&self) -> u32 {
        self.state.lock_or_recover().cursor.saturating_sub(1)
    }

    /// Writing `ns_last_pid` (`pid_ns_ctl_handler`): the next number is searched for from one
    /// past `last`, which must be at most `pid_max`.
    pub(crate) fn set_last_pid(&self, last: u32) -> Result<(), Errno> {
        let mut state = self.state.lock_or_recover();
        if last > state.pid_max {
            return Err(Errno::EINVAL);
        }
        state.cursor = last + 1;
        Ok(())
    }

    pub(crate) fn pid_max(&self) -> u32 {
        self.state.lock_or_recover().pid_max
    }

    /// Writing `pid_max`: between [`PID_MAX_MIN`] and [`PID_MAX_LIMIT`]
    /// (`proc_dointvec_minmax`).
    pub(crate) fn set_pid_max(&self, max: u32) -> Result<(), Errno> {
        if !(PID_MAX_MIN..=PID_MAX_LIMIT).contains(&max) {
            return Err(Errno::EINVAL);
        }
        self.state.lock_or_recover().pid_max = max;
        Ok(())
    }

    /// Whether new numbers are still handed out here: the namespace's init has not exited.
    pub(crate) fn adding(&self) -> bool {
        self.state.lock_or_recover().adding
    }

    /// `disable_pid_allocation` and the start of `zap_pid_ns_processes`: the reaper exited, so
    /// no number is handed out here any more, and its exit waits for the others to go.
    pub(crate) fn start_zap(&self) {
        let mut state = self.state.lock_or_recover();
        state.adding = false;
        state.zapping = true;
    }

    /// Whether the reaper's exit waits ([`PidSpace::start_zap`]) and some number here is not
    /// the reaper's own.
    pub(crate) fn zap_waits(&self) -> bool {
        let state = self.state.lock_or_recover();
        state.zapping && state.used.values().any(|&id| Some(id) != state.reaper)
    }

    /// `idr_alloc` (`tid` asked for) or `idr_alloc_cyclic` from the cursor, for `id`. The
    /// first number of a namespace must be 1, its init's.
    fn alloc(&self, id: u32, tid: Option<u32>) -> Result<u32, Errno> {
        let mut state = self.state.lock_or_recover();
        if !state.adding {
            return Err(Errno::ENOMEM);
        }
        // None is in use: the previous attempt to start the init failed.
        if state.used.is_empty() {
            state.cursor = 0;
        }
        let max = state.pid_max;
        let nr = match tid {
            Some(tid) if tid < 1 || tid >= max => return Err(Errno::EINVAL),
            Some(tid) if state.used.contains_key(&tid) => return Err(Errno::EEXIST),
            Some(tid) => tid,
            None => {
                // Pid 1 is the init's; after reaching the maximum, allocation wraps back to
                // `RESERVED_PIDS`.
                let min = if state.cursor > RESERVED_PIDS {
                    RESERVED_PIDS
                } else {
                    1
                };
                let start = state.cursor.max(min);
                state
                    .first_free(start, max)
                    .or_else(|| state.first_free(min, max))
                    .ok_or(Errno::EAGAIN)?
            }
        };
        if state.reaper.is_none() && nr != 1 {
            return Err(Errno::EINVAL);
        }
        state.used.insert(nr, id);
        if tid.is_none() {
            state.cursor = nr + 1;
        }
        Ok(nr)
    }

    /// `free_pid` of `nr`: whether that leaves the reaper's exit nothing to wait for.
    fn free(&self, nr: u32) -> bool {
        let mut state = self.state.lock_or_recover();
        state.used.remove(&nr);
        state.zapping && state.used.values().all(|&id| Some(id) == state.reaper)
    }
}

impl SpaceState {
    /// The lowest number from `from` below `to` not in use.
    fn first_free(&self, from: u32, to: u32) -> Option<u32> {
        let mut want = from;
        for &nr in self.used.range(from..to).map(|(nr, _)| nr) {
            if nr != want {
                break;
            }
            want += 1;
        }
        (want < to).then_some(want)
    }
}

/// A task's number in one pid namespace (`struct upid`).
#[derive(Clone)]
pub(crate) struct Upid {
    pub(crate) nr: u32,
    pub(crate) ns: Arc<PidNs>,
}

/// What an id is attached to (`enum pid_type`): its numbers stay while any is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PidType {
    /// The task with the id.
    Pid,
    /// The thread group it leads, until reaped.
    Tgid,
    /// A process in the process group it names.
    Pgid,
    /// A process in the session it names.
    Sid,
}

/// The numbers of an id and what holds them.
struct Entry {
    /// One per level, the initial namespace's first (`numbers`).
    numbers: Box<[Upid]>,
    /// How many of each [`PidType`] are attached.
    users: [u32; 4],
    /// How many [`PidRef`]s hold the id (the `struct pid` count): they keep the entry, so
    /// its numbers can still be read, once nothing is attached.
    refs: u32,
    /// Nothing is attached any more: the numbers left their namespaces (`free_pid`), so no
    /// lookup finds the id and the numbers may name other ids.
    freed: bool,
}

/// The numbers of every id the kernel handed out and that something is attached to or a
/// [`PidRef`] holds.
#[derive(Default)]
pub(crate) struct Pids {
    table: RwLock<HashMap<u32, Entry>>,
    /// Namespaces whose reaper's exit waited for the number just freed
    /// ([`Kernel::pid_ns_progress`]).
    zapped: Mutex<Vec<Arc<PidNs>>>,
}

impl Pids {
    /// `alloc_pid` for the new task `id` in `ns`: a number there and in each ancestor, the
    /// innermost ones as `set_tid` asks (`clone3`), which needs `may_set` of each namespace
    /// asked for (`checkpoint_restore_ns_capable` over its owner, `EPERM`), attached to the task. `EINVAL` for more
    /// numbers asked than there are levels or one out of range, `ENOMEM` once the namespace
    /// or an ancestor lost its init, `EAGAIN` when one has no number left, `EEXIST` for an
    /// asked number in use.
    pub(crate) fn alloc(
        &self,
        id: u32,
        ns: &Arc<PidNs>,
        set_tid: &[i32],
        may_set: impl Fn(&Arc<PidNs>) -> bool,
    ) -> Result<(), Errno> {
        if set_tid.len() > ns.level + 1 {
            return Err(Errno::EINVAL);
        }
        let mut chain = Vec::with_capacity(ns.level + 1);
        let mut at = Some(ns.clone());
        while let Some(space) = at {
            at = space.parent.clone();
            chain.push(space);
        }
        let mut asked = Vec::with_capacity(set_tid.len());
        for (space, &tid) in chain.iter().zip(set_tid) {
            let tid = u32::try_from(tid)
                .ok()
                .filter(|&tid| tid >= 1 && tid < space.pid_max());
            asked.push(tid.ok_or(Errno::EINVAL)?);
            if !may_set(space) {
                return Err(Errno::EPERM);
            }
        }
        let mut taken: Vec<Upid> = Vec::with_capacity(chain.len());
        for (depth, space) in chain.iter().enumerate() {
            match space.alloc(id, asked.get(depth).copied()) {
                Ok(nr) => taken.push(Upid {
                    nr,
                    ns: space.clone(),
                }),
                Err(errno) => {
                    for upid in &taken {
                        upid.ns.free(upid.nr);
                    }
                    return Err(errno);
                }
            }
        }
        // `is_child_reaper`: pid 1 of its own namespace is the namespace's init.
        if taken[0].nr == 1 {
            ns.state.lock_or_recover().reaper = Some(id);
        }
        taken.reverse();
        let entry = Entry {
            numbers: taken.into_boxed_slice(),
            users: [1, 0, 0, 0],
            refs: 0,
            freed: false,
        };
        self.table.write_or_recover().insert(id, entry);
        Ok(())
    }

    /// Frees the numbers [`Pids::alloc`] gave `id` in `ns` and above for a process that is not
    /// made after all (`bad_fork_free_pid`): `ns` has no init again if it was to be its first.
    pub(crate) fn free_unused(&self, id: u32, ns: &PidNs) {
        {
            let mut state = ns.state.lock_or_recover();
            if state.reaper == Some(id) {
                state.reaper = None;
            }
        }
        self.detach(id, PidType::Pid);
    }

    /// Attaches one more `ty` to `id`.
    pub(crate) fn attach(&self, id: u32, ty: PidType) {
        if let Some(entry) = self.table.write_or_recover().get_mut(&id)
            && !entry.freed
        {
            entry.users[ty as usize] += 1;
        }
    }

    /// Detaches one `ty` from `id`; its numbers are free once nothing is attached
    /// (`__change_pid`, `free_pid`).
    pub(crate) fn detach(&self, id: u32, ty: PidType) {
        let freed = {
            let mut table = self.table.write_or_recover();
            let Some(entry) = table.get_mut(&id) else {
                return;
            };
            let users = &mut entry.users[ty as usize];
            *users = users.saturating_sub(1);
            if entry.freed || entry.users.iter().any(|&n| n != 0) {
                return;
            }
            match entry.refs {
                0 => table.remove(&id).expect("looked up above").numbers,
                _ => {
                    entry.freed = true;
                    entry.numbers.clone()
                }
            }
        };
        for upid in freed.iter() {
            if upid.ns.free(upid.nr) {
                self.zapped.lock_or_recover().push(upid.ns.clone());
            }
        }
    }

    /// Whether something of kind `ty` is attached to `id` (`pid_task(pid, type)` is not
    /// `NULL`): the task with the id, the thread group it leads, a process in the group or
    /// session it names.
    pub(crate) fn attached(&self, id: u32, ty: PidType) -> bool {
        let table = self.table.read_or_recover();
        table.get(&id).is_some_and(|e| e.users[ty as usize] > 0)
    }

    /// `pid_nr_ns`: the number `id` has in `ns`, 0 when it has none there or is gone.
    pub(crate) fn nr(&self, ns: &PidNs, id: u32) -> Pid {
        self.number(ns, id, false)
    }

    /// The number `id` has in `ns`; with `held`, also once the id is gone, as long as a
    /// [`PidRef`] keeps it.
    fn number(&self, ns: &PidNs, id: u32, held: bool) -> Pid {
        let table = self.table.read_or_recover();
        let entry = table.get(&id).filter(|entry| held || !entry.freed);
        let Some(upid) = entry.and_then(|e| e.numbers.get(ns.level)) else {
            return 0;
        };
        match std::ptr::eq(&**upid.ns, &**ns) {
            true => upid.nr as Pid,
            false => 0,
        }
    }

    /// The number `id` has in its own pid namespace, the innermost it is in (`pid_vnr` for a
    /// task of the caller's own thread group), 0 once it is gone.
    pub(crate) fn own_nr(&self, id: u32) -> Pid {
        let table = self.table.read_or_recover();
        let entry = table.get(&id).filter(|entry| !entry.freed);
        let last = entry.and_then(|entry| entry.numbers.last());
        last.map_or(0, |upid| upid.nr as Pid)
    }

    /// The numbers of `id` from `ns` down to its own namespace, `ns`'s first (the `NSpid:`
    /// line); empty when it has none in `ns`.
    pub(crate) fn upids_from(&self, ns: &PidNs, id: u32) -> Vec<Upid> {
        let table = self.table.read_or_recover();
        let Some(entry) = table.get(&id).filter(|entry| !entry.freed) else {
            return Vec::new();
        };
        match entry.numbers.get(ns.level) {
            Some(upid) if std::ptr::eq(&**upid.ns, &**ns) => entry.numbers[ns.level..].to_vec(),
            _ => Vec::new(),
        }
    }

    /// The namespaces whose reaper's exit may go on now ([`PidSpace::free`]).
    fn take_zapped(&self) -> Vec<Arc<PidNs>> {
        std::mem::take(&mut *self.zapped.lock_or_recover())
    }

    /// `get_pid`: a reference to `id` that keeps its numbers readable ([`Pids::nr`]) for as long
    /// as it lives, though the id may be gone by then. An id the table no longer has reads 0.
    pub(crate) fn get(self: &Arc<Self>, id: u32) -> PidRef {
        if let Some(entry) = self.table.write_or_recover().get_mut(&id) {
            entry.refs += 1;
        }
        PidRef {
            pids: self.clone(),
            id,
        }
    }

    /// `put_pid`: one reference to `id` less; a freed id's entry goes with its last one.
    fn put(&self, id: u32) {
        let mut table = self.table.write_or_recover();
        let Some(entry) = table.get_mut(&id) else {
            return;
        };
        entry.refs = entry.refs.saturating_sub(1);
        if entry.refs == 0 && entry.freed {
            table.remove(&id);
        }
    }
}

/// A held `struct pid`: what an object keeps of the process that last used it (a SysV IPC
/// object's creator or last operator), to show its number later, as Linux's objects do with
/// `get_pid` and `pid_vnr`, even after the process was reaped.
pub(crate) struct PidRef {
    pids: Arc<Pids>,
    id: u32,
}

impl PidRef {
    /// The kernel's id it holds.
    pub(crate) fn id(&self) -> u32 {
        self.id
    }

    /// A reference to another id of the same table.
    pub(crate) fn of(&self, id: u32) -> PidRef {
        self.pids.get(id)
    }

    /// `pid_nr_ns`: its number in `ns`, 0 when it has none there.
    pub(crate) fn nr(&self, ns: &PidNs) -> Pid {
        self.pids.number(ns, self.id, true)
    }
}

impl Clone for PidRef {
    fn clone(&self) -> Self {
        self.pids.get(self.id)
    }
}

impl Drop for PidRef {
    fn drop(&mut self) {
        self.pids.put(self.id);
    }
}

impl std::fmt::Debug for PidRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PidRef").field(&self.id).finish()
    }
}

impl Kernel {
    /// The pid namespace of `task`'s process (`task_active_pid_ns`).
    pub(crate) fn active_pid_ns(&self, task: TaskId) -> Arc<PidNs> {
        self.task_ref(task, |t| t.process.pid_ns.clone())
    }

    /// `pid_vnr`: the id `id` as the caller sees it, 0 when it is outside the caller's pid
    /// namespace (or gone).
    pub(crate) fn vnr(&self, caller: &Caller<'_>, id: u32) -> Pid {
        self.pids.nr(&self.active_pid_ns(caller.task()), id)
    }

    /// `find_vpid`: the id the caller names `nr`.
    pub(crate) fn find_vpid(&self, caller: &Caller<'_>, nr: Pid) -> Option<u32> {
        self.active_pid_ns(caller.task()).find(nr)
    }

    /// `task_pid_vnr(current)`: `task`'s tid as it sees it itself.
    pub(crate) fn own_tid(&self, task: TaskId) -> Pid {
        self.pids.own_nr(task.0)
    }

    /// `zap_pid_ns_processes` for `process`, whose thread group exited, if it is the init of a
    /// pid namespace below the initial one: no process starts in the namespace any more, the
    /// init ignores SIGCHLD and its zombies are reaped, so its children go as they exit, and
    /// every other process with a pid in the namespace is sent SIGKILL. Returns whether the
    /// init waits for them before it is a zombie itself, so that when its parent reaps it,
    /// the namespace is empty: [`Kernel::pid_ns_progress`] goes on once they are gone.
    pub(crate) fn zap_pid_ns(&self, process: &Arc<crate::Process>) -> bool {
        let ns = &process.pid_ns;
        let init = process.pid.0;
        if ns.level() == 0 || ns.reaper() != Some(init) {
            return false;
        }
        if ns.adding() {
            ns.start_zap();
            if let Some(signals) = self.jobs.group_signals(init) {
                signals.ignore_sigchld();
            }
            // `kernel_wait4` for the zombies it has.
            {
                let mut tree = self.procs.tree.lock_or_recover();
                let zombies: Vec<u32> = tree
                    .nodes
                    .iter()
                    .filter(|(_, node)| node.parent == init && node.is_zombie())
                    .map(|(&pid, _)| pid)
                    .collect();
                for pid in zombies {
                    if let Some(node) = tree.reap(pid) {
                        process.add_child_usage(&node.process);
                    }
                }
            }
            // `group_send_sig_info(SIGKILL, SEND_SIG_PRIV)` to every task of the namespace.
            for (_, id) in ns.members() {
                if id == init {
                    continue;
                }
                if let Some(signals) = self.jobs.group_signals(id) {
                    signals.send_kernel(wasixv2_abi::Signal::SIGKILL.0);
                }
            }
        }
        ns.zap_waits()
    }

    /// The end of `zap_pid_ns_processes` for every namespace whose last number besides its
    /// reaper's went since: the reaper's exit goes on ([`Kernel::exit_notify`]). Called with
    /// nothing locked, after anything that may free numbers.
    pub(crate) fn pid_ns_progress(&self) {
        for ns in self.pids.take_zapped() {
            let Some(reaper) = ns.reaper() else {
                continue;
            };
            if let Some(process) = self.procs.find(reaper) {
                self.exit_notify(&process);
            }
        }
    }
}
