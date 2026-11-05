//! System V semaphore sets (`ipc/sem.c`): `semget`, `semop`/`semtimedop`, `semctl` and
//! `SEM_UNDO` adjustments applied when a process exits.
//!
//! A set's operations run under its lock, all or nothing, as `perform_atomic_semop` does. An
//! operation that cannot proceed queues on the set in arrival order; after every change the
//! queue is rescanned (`update_queue`), and a sleeper finds its result in its queue entry.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};

use wasixv2_abi::{
    ConstKernelTimespecPtr, ConstSembufArray, GuestLayout, GuestPtr, GuestPtrMut, IpcCmd, IpcFlags,
    SemFlags, Sembuf, Semid64Ds,
};
use wasixv2_syscalls::{Caller, Errno, SysResult, read_value, write_value};
use wasixv2_wait::{PollMask, WaitQueue, wait_event, wait_event_timeout};

use super::{Ids, Object, Perm, READ, WRITE, get_or_create, split_time};
use crate::{Kernel, Process, pid::PidRef, sync::MutexExt, time};

/// `SEMMSL`: semaphores per set.
pub(crate) const SEMMSL: i32 = 32000;
/// `SEMMNS`: semaphores in the namespace.
pub(crate) const SEMMNS: i32 = SEMMNI * SEMMSL;
/// `SEMOPM`: operations per `semop`.
pub(crate) const SEMOPM: u32 = 500;
/// `SEMMNI`: sets in the namespace.
pub(crate) const SEMMNI: i32 = 32000;
/// `SEMVMX`: the largest value a semaphore holds.
pub(crate) const SEMVMX: i32 = 32767;
/// `SEMAEM`: the largest adjustment `SEM_UNDO` accumulates.
const SEMAEM: i32 = SEMVMX;
const SEMUME: i32 = SEMOPM as i32;
const SEMMNU: i32 = SEMMNS;
const SEMMAP: i32 = SEMMNS;
const SEMUSZ: i32 = 20;

/// The namespace's semaphore sets.
#[derive(Default)]
pub(crate) struct Sems {
    ids: Mutex<Ids<SemSet>>,
}

pub(crate) struct SemSet {
    state: Mutex<State>,
    perm: Mutex<Perm>,
    /// Woken whenever queued operations complete or the set goes away.
    queue: WaitQueue,
}

impl Object for SemSet {
    fn perm(&self) -> MutexGuard<'_, Perm> {
        self.perm.lock_or_recover()
    }
}

struct State {
    removed: bool,
    sems: Vec<Sem>,
    otime: u64,
    ctime: u64,
    pending: Vec<Arc<Pending>>,
    /// `struct sem_undo`: each process's adjustments, applied when it exits.
    undo: HashMap<u32, Vec<i32>>,
}

#[derive(Clone, Default)]
struct Sem {
    value: i32,
    /// The last process that changed the value (`sempid`).
    pid: Option<PidRef>,
}

/// `struct sem_queue`: a sleeping `semop`.
struct Pending {
    sops: Vec<Sembuf>,
    pid: PidRef,
    alter: bool,
    /// The operation the last attempt stopped at, for `GETNCNT` and `GETZCNT`.
    blocking: Mutex<usize>,
    /// The result, once another operation completed this one or the set went away.
    status: Mutex<Option<Result<(), Errno>>>,
}

/// Why an attempt did not complete.
enum Stop {
    /// The operation at this index would block.
    Block(usize),
    Fail(Errno),
}

impl State {
    /// `perform_atomic_semop`: applies `sops` in order on a copy and commits only if all of them
    /// can go, which also covers several operations on one semaphore.
    fn apply(&mut self, sops: &[Sembuf], owner: &PidRef) -> Result<(), Stop> {
        let pid = owner.id();
        let mut values: Vec<i32> = self.sems.iter().map(|sem| sem.value).collect();
        let undoes = sops
            .iter()
            .any(|sop| SemFlags(sop.sem_flg as u16 as u32).contains(SemFlags::SEM_UNDO));
        let mut adj = undoes.then(|| {
            self.undo
                .get(&pid)
                .cloned()
                .unwrap_or_else(|| vec![0; self.sems.len()])
        });
        for (index, sop) in sops.iter().enumerate() {
            let num = sop.sem_num as usize;
            let op = i32::from(sop.sem_op);
            let value = values[num];
            if op == 0 && value != 0 {
                return Err(Stop::Block(index));
            }
            let value = value + op;
            if value < 0 {
                return Err(Stop::Block(index));
            }
            if value > SEMVMX {
                return Err(Stop::Fail(Errno::ERANGE));
            }
            if SemFlags(sop.sem_flg as u16 as u32).contains(SemFlags::SEM_UNDO) {
                let adj = adj.as_mut().expect("undo array for SEM_UNDO");
                let undo = adj[num] - op;
                if !(-SEMAEM - 1..=SEMAEM).contains(&undo) {
                    return Err(Stop::Fail(Errno::ERANGE));
                }
                adj[num] = undo;
            }
            values[num] = value;
        }
        for (sem, value) in self.sems.iter_mut().zip(values) {
            sem.value = value;
        }
        for sop in sops {
            self.sems[sop.sem_num as usize].pid = Some(owner.clone());
        }
        if let Some(adj) = adj {
            self.undo.insert(pid, adj);
        }
        Ok(())
    }

    /// `update_queue`: completes every queued operation that can now go, in arrival order,
    /// rescanning after each one that changed a value. Returns whether any completed.
    fn update_queue(&mut self, now: u64) -> bool {
        let mut any = false;
        'scan: loop {
            let mut at = 0;
            while at < self.pending.len() {
                let pending = self.pending[at].clone();
                let result = match self.apply(&pending.sops, &pending.pid) {
                    Err(Stop::Block(index)) => {
                        *pending.blocking.lock_or_recover() = index;
                        at += 1;
                        continue;
                    }
                    Err(Stop::Fail(errno)) => Err(errno),
                    Ok(()) => Ok(()),
                };
                self.pending.remove(at);
                any = true;
                let altered = result.is_ok() && pending.alter;
                if result.is_ok() {
                    self.otime = now;
                }
                *pending.status.lock_or_recover() = Some(result);
                if altered {
                    continue 'scan;
                }
            }
            return any;
        }
    }

    /// `count_semcnt`: sleepers blocked on semaphore `num` waiting for it to grow (`zero` false)
    /// or reach zero.
    fn count_waiting(&self, num: usize, zero: bool) -> i32 {
        let blocked = |pending: &&Arc<Pending>| {
            let sop = pending.sops[*pending.blocking.lock_or_recover()];
            sop.sem_num as usize == num && (sop.sem_op == 0) == zero && sop.sem_op <= 0
        };
        self.pending.iter().filter(blocked).count() as i32
    }

    /// A `semctl` changed values: adjustments to them no longer apply, and sleepers may go.
    fn values_set(&mut self, nums: std::ops::Range<usize>, pid: &PidRef, now: u64) {
        for adj in self.undo.values_mut() {
            adj[nums.clone()].fill(0);
        }
        for sem in &mut self.sems[nums] {
            sem.pid = Some(pid.clone());
        }
        self.ctime = now;
        self.update_queue(now);
    }
}

impl SemSet {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock_or_recover()
    }
}

/// Takes a queued operation off its set if it is still there when the waiting call ends.
struct Queued<'a> {
    set: &'a SemSet,
    pending: Arc<Pending>,
}

impl Queued<'_> {
    /// The result another operation left, or `None` after taking the entry off the queue.
    fn finish(&self) -> Option<Result<(), Errno>> {
        let mut state = self.set.state();
        if let Some(result) = self.pending.status.lock_or_recover().take() {
            return Some(result);
        }
        state.pending.retain(|p| !Arc::ptr_eq(p, &self.pending));
        None
    }
}

impl Drop for Queued<'_> {
    fn drop(&mut self) {
        let mut state = self.set.state();
        state.pending.retain(|p| !Arc::ptr_eq(p, &self.pending));
    }
}

/// `struct seminfo`, ten `int`s.
fn seminfo(sets: i32, sems: i32, info: bool, limits: [i32; 4]) -> [i32; 10] {
    let (semusz, semaem) = match info {
        true => (sets, sems),
        false => (SEMUSZ, SEMAEM),
    };
    [
        SEMMAP, limits[3], limits[1], SEMMNU, limits[0], limits[2], SEMUME, semusz, SEMVMX, semaem,
    ]
}

impl Sems {
    fn get(&self, id: i32) -> Result<Arc<SemSet>, Errno> {
        self.ids.lock_or_recover().get(id).ok_or(Errno::EINVAL)
    }

    /// `exit_sem`: applies the adjustments process `pid` accumulated, clamped to the valid
    /// range, and wakes what they allow.
    pub(crate) fn exit(&self, owner: &PidRef, now: u64) {
        let sets = self.ids.lock_or_recover().all();
        for (_, set) in sets {
            let mut state = set.state();
            let Some(adj) = state.undo.remove(&owner.id()) else {
                continue;
            };
            for (sem, adj) in state.sems.iter_mut().zip(adj) {
                if adj != 0 {
                    sem.value = (sem.value + adj).clamp(0, SEMVMX);
                    sem.pid = Some(owner.clone());
                }
            }
            if state.update_queue(now) {
                set.queue.wake(PollMask::empty());
            }
        }
    }

    /// Whether process `pid` has adjustments in some set: whether it has a `sem_undo_list`,
    /// as `kcmp(KCMP_SYSVSEM)` compares them.
    pub(crate) fn has_undo(&self, pid: u32) -> bool {
        let sets = self.ids.lock_or_recover().all();
        sets.iter()
            .any(|(_, set)| set.state().undo.contains_key(&pid))
    }

    /// Rows of `/proc/sysvipc/sem`: id, permissions, number of semaphores, times.
    #[allow(
        dead_code,
        reason = "the provider for /proc/sysvipc/sem, for procfs to read"
    )]
    pub(crate) fn list(&self) -> Vec<SemInfo> {
        self.ids
            .lock_or_recover()
            .all()
            .into_iter()
            .map(|(id, set)| {
                let state = set.state();
                SemInfo {
                    id,
                    perm: set.perm().clone(),
                    nsems: state.sems.len() as u32,
                    otime: state.otime,
                    ctime: state.ctime,
                }
            })
            .collect()
    }
}

/// One semaphore set, as `/proc/sysvipc/sem` shows it.
#[allow(dead_code, reason = "read by procfs")]
pub(crate) struct SemInfo {
    pub(crate) id: i32,
    pub(crate) perm: Perm,
    pub(crate) nsems: u32,
    pub(crate) otime: u64,
    pub(crate) ctime: u64,
}

impl Kernel {
    pub(crate) fn sys_semget(
        &self,
        caller: &Caller<'_>,
        key: i32,
        nsems: i32,
        semflg: i32,
    ) -> SysResult<i32> {
        let cred = self.ipc_cred(caller);
        let now = self.ipc_now();
        let ns = self.ns(caller);
        let limits = ns.ipc.limits.lock_or_recover().sem;
        if !(0..=limits[0]).contains(&nsems) {
            return Err(Errno::EINVAL.into());
        }
        let sems = &ns.ipc.sem;
        let limit = limits[3] as usize;
        let check = |set: &SemSet| match nsems as usize > set.state().sems.len() {
            true => Err(Errno::EINVAL),
            false => Ok(()),
        };
        let create = |perm, ids: &Ids<SemSet>| {
            if nsems == 0 {
                return Err(Errno::EINVAL);
            }
            let used: i64 = ids
                .all()
                .iter()
                .map(|(_, set)| set.state().sems.len() as i64)
                .sum();
            if used + i64::from(nsems) > i64::from(limits[1]) {
                return Err(Errno::ENOSPC);
            }
            ids.check_capacity(limit)?;
            Ok(SemSet {
                state: Mutex::new(State {
                    removed: false,
                    sems: vec![Sem::default(); nsems as usize],
                    otime: 0,
                    ctime: now,
                    pending: Vec::new(),
                    undo: HashMap::new(),
                }),
                perm: Mutex::new(perm),
                queue: WaitQueue::new(),
            })
        };
        Ok(get_or_create(
            &sems.ids, key, semflg, &cred, limit, check, create,
        )?)
    }

    pub(crate) async fn sys_semtimedop(
        &self,
        caller: &Caller<'_>,
        semid: i32,
        tsops: ConstSembufArray,
        nsops: u32,
        timeout: ConstKernelTimespecPtr,
    ) -> SysResult<i32> {
        let timeout = match timeout.is_null() {
            true => None,
            false => Some(read_value(caller.memory(), timeout).await?),
        };
        let limits = self.ns(caller).ipc.limits.lock_or_recover().sem;
        if nsops > limits[2] as u32 {
            return Err(Errno::E2BIG.into());
        }
        if nsops < 1 {
            return Err(Errno::EINVAL.into());
        }
        let mut raw = vec![0; nsops as usize * Sembuf::SIZE];
        caller.memory().read(tsops.addr(), &mut raw).await?;
        let sops: Vec<Sembuf> = raw
            .chunks(Sembuf::SIZE)
            .map(|chunk| Sembuf::read_from(chunk).expect("chunk of the struct's size"))
            .collect();
        if semid < 0 {
            return Err(Errno::EINVAL.into());
        }
        let timeout = timeout.map(time::from_timespec).transpose()?;
        let max = sops.iter().map(|sop| sop.sem_num).max().unwrap_or(0) as usize;
        let alter = sops.iter().any(|sop| sop.sem_op != 0);
        let nowait =
            |sop: &Sembuf| IpcFlags(sop.sem_flg as u16 as u32).contains(IpcFlags::IPC_NOWAIT);
        let cred = self.ipc_cred(caller);
        let pid = self.pids.get(self.process(caller.task()).pid.0);
        let now = self.ipc_now();
        let set = self.ns(caller).ipc.sem.get(semid)?;
        let pending = {
            let mut state = set.state();
            if max >= state.sems.len() {
                return Err(Errno::EFBIG.into());
            }
            set.perm().check(&cred, if alter { WRITE } else { READ })?;
            if state.removed {
                return Err(Errno::EIDRM.into());
            }
            match state.apply(&sops, &pid) {
                Ok(()) => {
                    state.otime = now;
                    if alter && state.update_queue(now) {
                        set.queue.wake(PollMask::empty());
                    }
                    return Ok(0);
                }
                Err(Stop::Fail(errno)) => return Err(errno.into()),
                Err(Stop::Block(index)) if nowait(&sops[index]) => {
                    return Err(Errno::EAGAIN.into());
                }
                Err(Stop::Block(index)) => {
                    let pending = Arc::new(Pending {
                        sops,
                        pid,
                        alter,
                        blocking: Mutex::new(index),
                        status: Mutex::new(None),
                    });
                    state.pending.push(pending.clone());
                    pending
                }
            }
        };
        let queued = Queued { set: &set, pending };
        let ready = || queued.pending.status.lock_or_recover().take();
        let interrupt = self.interrupt(caller);
        let woken = match timeout {
            None => wait_event(&set.queue, ready, &*interrupt).await.map(Some),
            Some(after) => {
                let sleep = self.host.sleep(after);
                wait_event_timeout(&set.queue, ready, &*interrupt, sleep).await
            }
        };
        let result = match woken {
            Ok(Some(result)) => result,
            // Completed between the wakeup and the cleanup, or not at all.
            Ok(None) => queued.finish().unwrap_or(Err(Errno::EAGAIN)),
            Err(_) => queued.finish().unwrap_or(Err(Errno::EINTR)),
        };
        Ok(result.map(|()| 0)?)
    }

    pub(crate) async fn sys_semctl(
        &self,
        caller: &Caller<'_>,
        semid: i32,
        semnum: i32,
        cmd: i32,
        arg: u32,
    ) -> SysResult<i32> {
        if semid < 0 {
            return Err(Errno::EINVAL.into());
        }
        let cred = self.ipc_cred(caller);
        let now = self.ipc_now();
        let pid = self.pids.get(self.process(caller.task()).pid.0);
        let ns = self.ns(caller);
        let sems = &ns.ipc.sem;
        match IpcCmd(cmd) {
            IpcCmd::IPC_INFO | IpcCmd::SEM_INFO => {
                let (info, max) = {
                    let ids = sems.ids.lock_or_recover();
                    let all = ids.all();
                    let used = all.iter().map(|(_, s)| s.state().sems.len() as i32).sum();
                    let sets = ids.in_use() as i32;
                    let limits = ns.ipc.limits.lock_or_recover().sem;
                    let info = seminfo(sets, used, IpcCmd(cmd) == IpcCmd::SEM_INFO, limits);
                    (info, ids.max_index())
                };
                write_ints(caller, arg, &info).await?;
                Ok(max)
            }
            IpcCmd::IPC_STAT | IpcCmd::SEM_STAT | super::SEM_STAT_ANY => {
                let (id, set) = match IpcCmd(cmd) {
                    IpcCmd::IPC_STAT => (semid, sems.get(semid)?),
                    _ => {
                        let set = sems.ids.lock_or_recover().at(semid).ok_or(Errno::EINVAL)?;
                        let id = super::build_id(semid as usize, set.perm().seq);
                        (id, set)
                    }
                };
                let ds = {
                    let state = set.state();
                    let perm = set.perm();
                    if IpcCmd(cmd) != super::SEM_STAT_ANY {
                        perm.check(&cred, READ)?;
                    }
                    let (otime, otime_high) = split_time(state.otime);
                    let (ctime, ctime_high) = split_time(state.ctime);
                    Semid64Ds {
                        sem_perm: perm.to_abi(&self.cred(caller).user_ns),
                        sem_otime: otime,
                        sem_otime_high: otime_high,
                        sem_ctime: ctime,
                        sem_ctime_high: ctime_high,
                        sem_nsems: state.sems.len() as u32,
                        ..Semid64Ds::default()
                    }
                };
                write_value(caller.memory(), GuestPtrMut::new(arg), &ds).await?;
                Ok(match IpcCmd(cmd) {
                    IpcCmd::IPC_STAT => 0,
                    _ => id,
                })
            }
            IpcCmd::IPC_SET => {
                let ds: Semid64Ds = read_value(caller.memory(), GuestPtr::new(arg)).await?;
                let set = sems.get(semid)?;
                let mut state = set.state();
                let mut perm = set.perm();
                perm.check_owner(&cred)?;
                perm.update(&ds.sem_perm, &self.cred(caller).user_ns)?;
                state.ctime = now;
                Ok(0)
            }
            IpcCmd::IPC_RMID => {
                let mut ids = sems.ids.lock_or_recover();
                let set = ids.get(semid).ok_or(Errno::EINVAL)?;
                set.perm().check_owner(&cred)?;
                ids.remove(semid);
                drop(ids);
                // `freeary`: every sleeper fails with EIDRM.
                let mut state = set.state();
                state.removed = true;
                for pending in std::mem::take(&mut state.pending) {
                    *pending.status.lock_or_recover() = Some(Err(Errno::EIDRM));
                }
                state.undo.clear();
                drop(state);
                set.queue.wake(PollMask::empty());
                Ok(0)
            }
            IpcCmd::SETVAL => {
                let val = arg as i32;
                if !(0..=SEMVMX).contains(&val) {
                    return Err(Errno::ERANGE.into());
                }
                let set = sems.get(semid)?;
                let mut state = set.state();
                let num = usize::try_from(semnum)
                    .ok()
                    .filter(|&num| num < state.sems.len())
                    .ok_or(Errno::EINVAL)?;
                set.perm().check(&cred, WRITE)?;
                if state.removed {
                    return Err(Errno::EIDRM.into());
                }
                state.sems[num].value = val;
                state.values_set(num..num + 1, &pid, now);
                drop(state);
                set.queue.wake(PollMask::empty());
                Ok(0)
            }
            IpcCmd::GETALL => {
                let set = sems.get(semid)?;
                set.perm().check(&cred, READ)?;
                let values: Vec<u8> = set
                    .state()
                    .sems
                    .iter()
                    .flat_map(|sem| (sem.value as u16).to_le_bytes())
                    .collect();
                caller.memory().write(arg, &values).await?;
                Ok(0)
            }
            IpcCmd::SETALL => {
                let set = sems.get(semid)?;
                set.perm().check(&cred, WRITE)?;
                let n = set.state().sems.len();
                let mut raw = vec![0; n * 2];
                caller.memory().read(arg, &mut raw).await?;
                let values: Vec<i32> = raw
                    .chunks(2)
                    .map(|v| i32::from(u16::from_le_bytes([v[0], v[1]])))
                    .collect();
                if values.iter().any(|&v| v > SEMVMX) {
                    return Err(Errno::ERANGE.into());
                }
                let mut state = set.state();
                if state.removed {
                    return Err(Errno::EIDRM.into());
                }
                for (sem, value) in state.sems.iter_mut().zip(values) {
                    sem.value = value;
                }
                state.values_set(0..n, &pid, now);
                drop(state);
                set.queue.wake(PollMask::empty());
                Ok(0)
            }
            IpcCmd::GETVAL | IpcCmd::GETPID | IpcCmd::GETNCNT | IpcCmd::GETZCNT => {
                // The last operator as the caller sees it (`pid_vnr`).
                let pid_ns = self.active_pid_ns(caller.task());
                let set = sems.get(semid)?;
                set.perm().check(&cred, READ)?;
                let state = set.state();
                let num = usize::try_from(semnum)
                    .ok()
                    .filter(|&num| num < state.sems.len())
                    .ok_or(Errno::EINVAL)?;
                Ok(match IpcCmd(cmd) {
                    IpcCmd::GETVAL => state.sems[num].value,
                    IpcCmd::GETPID => state.sems[num].pid.as_ref().map_or(0, |p| p.nr(&pid_ns)),
                    IpcCmd::GETNCNT => state.count_waiting(num, false),
                    _ => state.count_waiting(num, true),
                })
            }
            _ => Err(Errno::EINVAL.into()),
        }
    }

    /// `exit_sem` for a process that is gone, in the IPC namespace its adjustments are in.
    pub(crate) fn ipc_process_exited(&self, process: &Process) {
        if let Some(ipc) = process.sysvsem.lock_or_recover().take() {
            ipc.sem.exit(&self.pids.get(process.pid.0), self.ipc_now());
        }
    }
}

/// Copies an `*info` structure of `int`s out to `addr`.
pub(super) async fn write_ints(caller: &Caller<'_>, addr: u32, ints: &[i32]) -> SysResult<()> {
    let bytes: Vec<u8> = ints.iter().flat_map(|v| v.to_le_bytes()).collect();
    caller.memory().write(addr, &bytes).await?;
    Ok(())
}
