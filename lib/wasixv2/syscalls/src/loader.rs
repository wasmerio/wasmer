//! Engine-free admission and store rendezvous for an mm's loader transaction.
//! Linux's task_work/exit-to-user boundary is the wakeup sibling; no guest signal is used.

use crate::{Errno, Layered, TaskId};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, MutexGuard, Weak},
};
use wasixv2_wait::{NoInterrupt, PollMask, WaitQueue, wait_event};

/// Store-local work. Advancing requires an acknowledgment from every surviving participant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoaderPhase {
    Park,
    Prepare,
    Apply,
    Publish,
    Rollback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoaderCommand {
    pub epoch: u64,
    pub phase: LoaderPhase,
}

/// An address-space identity and transaction. It contains no wasm objects or guest pointers.
pub struct LoaderGate {
    mm: u64,
    state: Mutex<State>,
    changed: WaitQueue,
}

#[derive(Default)]
struct State {
    image_owner: Weak<()>,
    generation: u64,
    next_epoch: u64,
    admissions: usize,
    stores: BTreeSet<u32>,
    closing: Option<(u64, TaskId)>,
    round: Option<Round>,
}

struct Round {
    epoch: u64,
    owner: TaskId,
    coordinated: bool,
    coordinator: Arc<()>,
    phase: LoaderPhase,
    participants: BTreeMap<u32, bool>,
}

impl LoaderGate {
    pub fn new(mm: u64) -> Arc<Self> {
        Arc::new(Self {
            mm,
            state: Mutex::default(),
            changed: WaitQueue::default(),
        })
    }
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn wake(&self) {
        self.changed.wake(PollMask::empty());
    }
    pub fn mm(&self) -> u64 {
        self.mm
    }
    pub fn generation(&self) -> u64 {
        self.state().generation
    }

    /// Held across clone/exec's preflight and membership update. Closed admission returns
    /// EAGAIN to kernel glue, which must restart at a loader-aware guest safe point, not
    /// wait inside a syscall while the coordinator is waiting for that same task to park.
    pub fn admit(self: &Arc<Self>) -> Result<LoaderAdmission, Errno> {
        self.admit_locked(&mut self.state(), None)
    }

    /// Claim an mm's graph identity while entering existing loader admission. The mm
    /// retains only a weak nonce; images/stores and this in-flight admission retain it
    /// strongly until cleanup finishes. A different live graph is never substituted.
    pub fn claim_image(self: &Arc<Self>, owner: &Arc<()>) -> Result<LoaderAdmission, Errno> {
        self.admit_locked(&mut self.state(), Some(owner))
    }

    fn admit_locked(
        self: &Arc<Self>,
        state: &mut State,
        owner: Option<&Arc<()>>,
    ) -> Result<LoaderAdmission, Errno> {
        if state.closing.is_some() || state.round.is_some() {
            return Err(Errno::EAGAIN);
        }
        let admissions = state.admissions.checked_add(1).ok_or(Errno::EAGAIN)?;
        if let Some(owner) = owner {
            if let Some(current) = state.image_owner.upgrade()
                && !Arc::ptr_eq(&current, owner)
            {
                return Err(Errno::EBUSY);
            }
            state.image_owner = Arc::downgrade(owner);
        }
        state.admissions = admissions;
        Ok(LoaderAdmission {
            gate: self.clone(),
            active: true,
            _image: owner.cloned(),
        })
    }

    /// Closes admission first; after existing admissions drain the kernel snapshots members
    /// under its task-table lock and activates the round under that same lock.
    pub fn close(self: &Arc<Self>, owner: TaskId) -> Result<LoaderClosing, Errno> {
        let mut state = self.state();
        if state.closing.is_some() || state.round.is_some() {
            return Err(Errno::EAGAIN);
        }
        // Generation overflow must fail before any store enters the publication boundary.
        if state.generation == u64::MAX {
            return Err(Errno::EOVERFLOW);
        }
        state.next_epoch = state.next_epoch.checked_add(1).ok_or(Errno::EOVERFLOW)?;
        let epoch = state.next_epoch;
        state.closing = Some((epoch, owner));
        Ok(LoaderClosing {
            gate: self.clone(),
            epoch,
            owner,
            active: true,
        })
    }

    /// The work not yet acknowledged by this task; no command means it may proceed normally.
    pub fn command(&self, task: TaskId) -> Option<LoaderCommand> {
        let state = self.state();
        let round = state.round.as_ref()?;
        (!*round.participants.get(&task.0)?).then_some(LoaderCommand {
            epoch: round.epoch,
            phase: round.phase,
        })
    }

    /// Current surviving store participants. Unstarted tasks stay entry-barred and
    /// never appear as stores; exit removes its participant under the same state lock.
    pub fn participants(&self) -> Vec<TaskId> {
        self.state().round.as_ref().map_or_else(Vec::new, |round| {
            round.participants.keys().map(|id| TaskId(*id)).collect()
        })
    }

    pub fn acknowledge(&self, task: TaskId, command: LoaderCommand) -> Result<(), Errno> {
        let mut state = self.state();
        let round = state.round.as_mut().ok_or(Errno::ESTALE)?;
        if round.epoch != command.epoch || round.phase != command.phase {
            return Err(Errno::ESTALE);
        }
        *round.participants.get_mut(&task.0).ok_or(Errno::ESRCH)? = true;
        drop(state);
        self.wake();
        Ok(())
    }

    /// Membership detachment is called while the kernel removes the task from this mm.
    /// Owner death changes the operation to rollback; store-local cleanup precedes its ack.
    pub fn leave(&self, task: TaskId) {
        let mut state = self.state();
        state.stores.remove(&task.0);
        if let Some(round) = &mut state.round {
            round.participants.remove(&task.0);
            if round.owner == task && round.phase != LoaderPhase::Publish {
                round.phase = LoaderPhase::Rollback;
                round.coordinated = false;
                round.participants.values_mut().for_each(|ack| *ack = false);
            }
        }
        drop(state);
        self.wake();
    }

    /// Waits for a changed command or release. Callers service exits while parking; they do
    /// not execute guest signal handlers or return to guest until the transaction releases.
    pub async fn wait_change(&self, task: TaskId, previous: Option<LoaderCommand>) {
        let _ = wait_event(
            &self.changed,
            || {
                let command = self.command(task);
                (command != previous || !self.busy()).then_some(())
            },
            &NoInterrupt,
        )
        .await;
    }

    /// Admission for a new store. A task without a store is behind this entry barrier and
    /// need not be fabricated as an existing store during a rendezvous. Once this returns,
    /// closing waits for the caller to finish binding the complete committed graph.
    pub fn bind(self: &Arc<Self>, task: TaskId) -> Result<LoaderAdmission, Errno> {
        let admission = self.admit()?;
        self.state().stores.insert(task.0);
        Ok(admission)
    }

    pub fn queue(&self) -> &WaitQueue {
        &self.changed
    }

    pub fn ready_for(&self, wait: LoaderWait) -> bool {
        match wait {
            LoaderWait::Drained => self.state().admissions == 0,
            LoaderWait::Idle => !self.busy(),
            LoaderWait::Command { task, previous } => {
                self.command(task) != previous || !self.busy()
            }
            LoaderWait::Acknowledged { epoch } => self.state().round.as_ref().is_none_or(|round| {
                round.epoch != epoch || round.participants.values().all(|ack| *ack)
            }),
        }
    }

    /// A surviving participant adopts abandoned rollback; only one cleanup coordinator wins.
    pub fn recover(self: &Arc<Self>, task: TaskId) -> Option<LoaderTransaction> {
        let mut state = self.state();
        let round = state.round.as_mut()?;
        if !matches!(round.phase, LoaderPhase::Rollback | LoaderPhase::Publish) || round.coordinated
        {
            return None;
        }
        round.coordinated = true;
        round.coordinator = Arc::new(());
        round.owner = task;
        Some(LoaderTransaction {
            gate: self.clone(),
            epoch: round.epoch,
            coordinator: round.coordinator.clone(),
            active: true,
        })
    }

    pub fn busy(&self) -> bool {
        let state = self.state();
        state.closing.is_some() || state.round.is_some()
    }
}

pub struct LoaderAdmission {
    gate: Arc<LoaderGate>,
    active: bool,
    _image: Option<Arc<()>>,
}
impl Drop for LoaderAdmission {
    fn drop(&mut self) {
        if self.active {
            let mut state = self.gate.state();
            state.admissions -= 1;
            drop(state);
            self.gate.wake();
        }
    }
}

pub struct LoaderClosing {
    gate: Arc<LoaderGate>,
    epoch: u64,
    owner: TaskId,
    active: bool,
}
impl LoaderClosing {
    pub fn gate(&self) -> &Arc<LoaderGate> {
        &self.gate
    }
    pub async fn drained(&self) {
        let _ = wait_event(
            &self.gate.changed,
            || (self.gate.state().admissions == 0).then_some(()),
            &NoInterrupt,
        )
        .await;
    }

    /// Kernel-only membership snapshot boundary: call while holding the task-table lock.
    /// It must include unstarted tasks and all CLONE_VM groups, and reject borrowed-vfork
    /// instances before activation. Dropping a rejected closing token simply reopens admission.
    pub fn activate(mut self, participants: &[TaskId]) -> Result<LoaderTransaction, Errno> {
        let mut state = self.gate.state();
        if state.closing != Some((self.epoch, self.owner))
            || state.admissions != 0
            || !participants.contains(&self.owner)
        {
            return Err(Errno::EBUSY);
        }
        if !state.stores.contains(&self.owner.0) {
            return Err(Errno::EBUSY);
        }
        let stores = participants
            .iter()
            .filter(|task| state.stores.contains(&task.0))
            .map(|task| (task.0, false))
            .collect();
        let coordinator = Arc::new(());
        state.round = Some(Round {
            epoch: self.epoch,
            owner: self.owner,
            coordinated: true,
            coordinator: coordinator.clone(),
            phase: LoaderPhase::Park,
            participants: stores,
        });
        state.closing = None;
        drop(state);
        self.active = false;
        self.gate.wake();
        Ok(LoaderTransaction {
            gate: self.gate.clone(),
            epoch: self.epoch,
            coordinator,
            active: true,
        })
    }
}
impl Drop for LoaderClosing {
    fn drop(&mut self) {
        if self.active {
            let mut state = self.gate.state();
            if state.closing == Some((self.epoch, self.owner)) {
                state.closing = None;
            }
            drop(state);
            self.gate.wake();
        }
    }
}

pub struct LoaderTransaction {
    gate: Arc<LoaderGate>,
    epoch: u64,
    coordinator: Arc<()>,
    active: bool,
}
impl LoaderTransaction {
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn gate(&self) -> &Arc<LoaderGate> {
        &self.gate
    }
    fn owns(&self, round: &Round) -> bool {
        round.epoch == self.epoch
            && round.coordinated
            && Arc::ptr_eq(&round.coordinator, &self.coordinator)
    }
    pub fn ready(&self) -> bool {
        self.gate
            .state()
            .round
            .as_ref()
            .is_some_and(|round| self.owns(round) && round.participants.values().all(|ack| *ack))
    }
    pub async fn wait_ready(&self) {
        let _ = wait_event(
            &self.gate.changed,
            || {
                self.gate
                    .state()
                    .round
                    .as_ref()
                    .is_none_or(|round| {
                        !self.owns(round) || round.participants.values().all(|ack| *ack)
                    })
                    .then_some(())
            },
            &NoInterrupt,
        )
        .await;
    }
    pub fn advance(&self, next: LoaderPhase) -> Result<(), Errno> {
        let mut state = self.gate.state();
        let round = state
            .round
            .as_mut()
            .filter(|round| self.owns(round))
            .ok_or(Errno::ESTALE)?;
        if !round.participants.values().all(|ack| *ack) {
            return Err(Errno::EBUSY);
        }
        let allowed = matches!(
            (round.phase, next),
            (LoaderPhase::Park, LoaderPhase::Prepare)
                | (LoaderPhase::Prepare, LoaderPhase::Apply)
                | (LoaderPhase::Apply, LoaderPhase::Publish)
        );
        if !allowed {
            return Err(Errno::EINVAL);
        }
        round.phase = next;
        round.participants.values_mut().for_each(|ack| *ack = false);
        drop(state);
        self.gate.wake();
        Ok(())
    }
    /// Release only after every store installed the published generation (or undid rollback).
    /// The live graph/output commit must be prepared before entering Publish.
    pub fn finish(mut self) -> Result<u64, Errno> {
        let mut state = self.gate.state();
        let round = state
            .round
            .as_ref()
            .filter(|round| self.owns(round))
            .ok_or(Errno::ESTALE)?;
        if !round.participants.values().all(|ack| *ack)
            || !matches!(round.phase, LoaderPhase::Publish | LoaderPhase::Rollback)
        {
            return Err(Errno::EBUSY);
        }
        if round.phase == LoaderPhase::Publish {
            state.generation = state.generation.checked_add(1).ok_or(Errno::EOVERFLOW)?;
        }
        let generation = state.generation;
        state.round = None;
        drop(state);
        self.active = false;
        self.gate.wake();
        Ok(generation)
    }
}
impl Drop for LoaderTransaction {
    fn drop(&mut self) {
        if self.active {
            let mut state = self.gate.state();
            if let Some(round) = state.round.as_mut().filter(|round| {
                round.epoch == self.epoch && Arc::ptr_eq(&round.coordinator, &self.coordinator)
            }) {
                // Publish is the non-failing boundary. Cleanup must finish that commit,
                // never report it as a recoverable dlopen error or roll it back.
                round.coordinated = false;
                if round.phase != LoaderPhase::Publish {
                    round.phase = LoaderPhase::Rollback;
                    round.coordinated = false;
                    round.participants.values_mut().for_each(|ack| *ack = false);
                }
            }
            drop(state);
            self.gate.wake();
        }
    }
}

/// Internal loader hooks, a Control supertrait so layering forwards them without modifying
/// generated syscall/WIT files. Defaults keep embedders without loader support unchanged.
#[derive(Clone, Copy)]
pub enum LoaderWait {
    Idle,
    Drained,
    Command {
        task: TaskId,
        previous: Option<LoaderCommand>,
    },
    Acknowledged {
        epoch: u64,
    },
}

/// An opened file image: its inode identity is held alive by the Image's backing owner.
#[derive(Clone)]
pub struct LibraryImage {
    pub identity: (u64, u64),
    pub image: crate::Image,
}

/// An ordinary private anonymous mm mapping, independent of the allocating task's later pid
/// or exec image. Dropping an unretained lease schedules its mm-specific cleanup; explicit
/// release can be awaited before reopening admission. Retained DSO data lasts until mm exit.
pub trait LoaderReservation: Send + Sync {
    fn address(&self) -> u32;
    fn length(&self) -> u32;
    fn retain(&mut self);
    fn release(self: Box<Self>) -> crate::LocalFuture<'static, ()>;
}

pub trait LoaderControl {
    fn loader_read<'a>(
        &'a self,
        caller: &'a crate::Caller<'_>,
        path: &'a [u8],
    ) -> crate::LocalFuture<'a, crate::SysResult<LibraryImage>> {
        let _ = (caller, path);
        Box::pin(async { Err(Errno::ENOSYS.into()) })
    }
    fn loader_reserve<'a>(
        &'a self,
        caller: &'a crate::Caller<'_>,
        bytes: u32,
        alignment: u32,
    ) -> crate::LocalFuture<'a, crate::SysResult<Box<dyn LoaderReservation>>> {
        let _ = (caller, bytes, alignment);
        Box::pin(async { Err(Errno::ENOSYS.into()) })
    }

    fn loader_gate(&self, task: TaskId) -> Option<Arc<LoaderGate>> {
        let _ = task;
        None
    }
    fn loader_activate(
        &self,
        task: TaskId,
        closing: LoaderClosing,
    ) -> Result<LoaderTransaction, Errno> {
        let _ = (task, closing);
        Err(Errno::ENOSYS)
    }
    fn loader_sleep<'a>(
        &'a self,
        task: TaskId,
        gate: &'a LoaderGate,
        wait: LoaderWait,
    ) -> crate::LocalFuture<'a, Result<(), Errno>> {
        let _ = task;
        Box::pin(async move {
            wait_event(
                gate.queue(),
                || gate.ready_for(wait).then_some(()),
                &NoInterrupt,
            )
            .await
            .map_err(|_| Errno::EINTR)
        })
    }
    fn loader_notify(&self, task: TaskId) {
        let _ = task;
    }
}
impl<L, S: LoaderControl> LoaderControl for Layered<L, S> {
    fn loader_read<'a>(
        &'a self,
        caller: &'a crate::Caller<'_>,
        path: &'a [u8],
    ) -> crate::LocalFuture<'a, crate::SysResult<LibraryImage>> {
        self.inner.loader_read(caller, path)
    }
    fn loader_reserve<'a>(
        &'a self,
        caller: &'a crate::Caller<'_>,
        bytes: u32,
        alignment: u32,
    ) -> crate::LocalFuture<'a, crate::SysResult<Box<dyn LoaderReservation>>> {
        self.inner.loader_reserve(caller, bytes, alignment)
    }

    fn loader_gate(&self, task: TaskId) -> Option<Arc<LoaderGate>> {
        self.inner.loader_gate(task)
    }
    fn loader_activate(
        &self,
        task: TaskId,
        closing: LoaderClosing,
    ) -> Result<LoaderTransaction, Errno> {
        self.inner.loader_activate(task, closing)
    }
    fn loader_sleep<'a>(
        &'a self,
        task: TaskId,
        gate: &'a LoaderGate,
        wait: LoaderWait,
    ) -> crate::LocalFuture<'a, Result<(), Errno>> {
        self.inner.loader_sleep(task, gate, wait)
    }
    fn loader_notify(&self, task: TaskId) {
        self.inner.loader_notify(task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exhausted_generation_rejects_before_publication() {
        let gate = LoaderGate::new(9);
        gate.state().generation = u64::MAX;
        assert!(matches!(gate.close(TaskId(1)), Err(Errno::EOVERFLOW)));
        assert!(!gate.busy());
        assert!(gate.admit().is_ok());
    }

    #[test]
    fn closing_admission_cannot_race_an_inflight_clone() {
        let gate = LoaderGate::new(1);
        drop(gate.bind(TaskId(1)).unwrap());
        drop(gate.bind(TaskId(2)).unwrap());
        let admission = gate.admit().unwrap();
        let closing = gate.close(TaskId(1)).unwrap();
        assert!(gate.admit().is_err());
        assert!(closing.activate(&[TaskId(1)]).is_err());
        assert!(!gate.busy());
        drop(admission);
        let closing = gate.close(TaskId(1)).unwrap();
        futures::executor::block_on(closing.drained());
        let round = closing.activate(&[TaskId(1), TaskId(2)]).unwrap();
        assert!(!round.ready());
        let cmd = gate.command(TaskId(1)).unwrap();
        gate.acknowledge(TaskId(1), cmd).unwrap();
        assert!(round.advance(LoaderPhase::Prepare).is_err());
        gate.acknowledge(TaskId(2), cmd).unwrap();
        round.advance(LoaderPhase::Prepare).unwrap();
        assert!(gate.acknowledge(TaskId(2), cmd).is_err());
        assert_eq!(gate.generation(), 0);
    }

    #[test]
    fn every_store_acknowledges_each_phase_before_release() {
        let gate = LoaderGate::new(2);
        drop(gate.bind(TaskId(7)).unwrap());
        drop(gate.bind(TaskId(8)).unwrap());
        let round = gate
            .close(TaskId(7))
            .unwrap()
            .activate(&[TaskId(7), TaskId(8)])
            .unwrap();
        for next in [
            LoaderPhase::Prepare,
            LoaderPhase::Apply,
            LoaderPhase::Publish,
        ] {
            let cmd = gate.command(TaskId(7)).unwrap();
            gate.acknowledge(TaskId(7), cmd).unwrap();
            assert!(round.advance(next).is_err());
            gate.acknowledge(TaskId(8), cmd).unwrap();
            round.advance(next).unwrap();
        }
        let cmd = gate.command(TaskId(7)).unwrap();
        gate.acknowledge(TaskId(7), cmd).unwrap();
        gate.acknowledge(TaskId(8), cmd).unwrap();
        assert_eq!(round.finish().unwrap(), 1);
        assert!(!gate.busy());
        assert!(gate.admit().is_ok());
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    #[test]
    fn a_stale_coordinator_cannot_release_an_adopted_rollback() {
        let gate = LoaderGate::new(5);
        drop(gate.bind(TaskId(1)).unwrap());
        drop(gate.bind(TaskId(2)).unwrap());
        let old = gate
            .close(TaskId(1))
            .unwrap()
            .activate(&[TaskId(1), TaskId(2)])
            .unwrap();
        gate.leave(TaskId(1));
        let cleanup = gate.recover(TaskId(2)).unwrap();
        gate.acknowledge(TaskId(2), gate.command(TaskId(2)).unwrap())
            .unwrap();
        assert_eq!(old.finish(), Err(Errno::ESTALE));
        assert!(gate.recover(TaskId(3)).is_none());
        assert_eq!(cleanup.finish().unwrap(), 0);
    }

    #[test]
    fn last_store_death_does_not_strand_a_future_entry() {
        let gate = LoaderGate::new(4);
        drop(gate.bind(TaskId(1)).unwrap());
        let transaction = gate
            .close(TaskId(1))
            .unwrap()
            .activate(&[TaskId(1), TaskId(2)])
            .unwrap();
        drop(transaction);
        gate.leave(TaskId(1));
        // Host cleanup can adopt even when no registered store survives; task 2 has not
        // constructed its store yet and must remain behind the entry barrier until release.
        assert!(gate.bind(TaskId(2)).is_err());
        let cleanup = gate.recover(TaskId(1)).unwrap();
        assert!(cleanup.ready());
        assert_eq!(cleanup.finish().unwrap(), 0);
        assert!(gate.bind(TaskId(2)).is_ok());
    }
}

#[cfg(test)]
mod image_owner_tests {
    use super::*;
    use std::sync::{
        Barrier,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn image_owner_lasts_through_all_attachments_and_inflight_admission() {
        let gate = LoaderGate::new(10);
        let image = Arc::new(());
        let store = image.clone();
        let weak = Arc::downgrade(&image);
        let admission = gate.claim_image(&image).unwrap();
        assert!(gate.claim_image(&store).is_ok());
        drop(image);
        drop(store);
        assert!(
            weak.upgrade().is_some(),
            "admitted initialization retains its owner"
        );
        assert!(matches!(gate.claim_image(&Arc::new(())), Err(Errno::EBUSY)));
        drop(admission);
        assert!(
            weak.upgrade().is_none(),
            "gate itself must not retain a strong owner"
        );
        assert!(gate.claim_image(&Arc::new(())).is_ok());
    }

    #[test]
    fn image_owner_claim_is_atomic_and_respects_closed_admission() {
        let gate = LoaderGate::new(11);
        let barrier = Arc::new(Barrier::new(3));
        let winners = Arc::new(AtomicUsize::new(0));
        let owners = [Arc::new(()), Arc::new(())];
        std::thread::scope(|scope| {
            for owner in &owners {
                let gate = gate.clone();
                let barrier = barrier.clone();
                let winners = winners.clone();
                scope.spawn(move || {
                    barrier.wait();
                    match gate.claim_image(owner) {
                        Ok(_admission) => {
                            winners.fetch_add(1, Ordering::SeqCst);
                        }
                        Err(errno) => assert_eq!(errno, Errno::EBUSY),
                    }
                });
            }
            barrier.wait();
        });
        assert_eq!(winners.load(Ordering::SeqCst), 1);
        let closing = gate.close(TaskId(1)).unwrap();
        assert!(matches!(gate.claim_image(&owners[0]), Err(Errno::EAGAIN)));
        drop(closing);
        // A fresh exec mm has independent ownership even while CLONE_VM attachments
        // retain the old mm's token; replacing it cannot steal the surviving graph.
        let fresh = LoaderGate::new(12);
        assert!(fresh.claim_image(&owners[1]).is_ok());
        assert_eq!(
            owners
                .iter()
                .filter(|owner| gate.claim_image(owner).is_ok())
                .count(),
            1
        );
    }
}

#[cfg(test)]
mod participant_snapshot_tests {
    use super::*;
    #[test]
    fn snapshot_excludes_unstarted_and_detached_stores() {
        let gate = LoaderGate::new(13);
        drop(gate.bind(TaskId(1)).unwrap());
        assert!(gate.participants().is_empty());
        let round = gate
            .close(TaskId(1))
            .unwrap()
            .activate(&[TaskId(1), TaskId(2)])
            .unwrap();
        assert_eq!(gate.participants(), [TaskId(1)]);
        gate.leave(TaskId(1));
        assert!(gate.participants().is_empty());
        drop(round);
    }
}
