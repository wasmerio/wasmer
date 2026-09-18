//! Shared dynamic-link state for every [`super::super::Linker`] clone.
//!
//! Owns [`LinkerState`] behind [`RwLock`], the topology [`TopologyCoordinator`], and the
//! `dl_operation_pending` [`AtomicBool`] used to coordinate [`LinkerShared::synchronize_link_operation`]
//! with followers. Prefer these helpers over raw lock calls — lock ordering and the “never block
//! on `write()` without cooperating” invariant are spelled out on [`super`] (the linker `sync`
//! module).

use std::{
    ops::Deref,
    sync::{
        Arc, RwLock, RwLockReadGuard, RwLockWriteGuard,
        atomic::{AtomicBool, Ordering},
    },
};

use tokio::sync::Barrier;
use tracing::trace;
use wasmer::{AsStoreMut, FunctionEnv, FunctionEnvMut};

use crate::WasiEnv;

use super::super::{InstanceGroupState, LinkError, LinkerState};
use super::{
    DlOperation, LinkerStateWriteBackoff,
    cancellation::LinkerCancellation,
    topology_lock::{TopologyCoordinator, TopologyToken},
};

#[cfg(all(test, feature = "sys-thread", not(target_arch = "wasm32")))]
mod tests;

/// Shared linkage and synchronization primitives for every [`super::super::Linker`] handle.
///
/// Cloning is cheap (`Arc`-backed locks and coordinators); clone when an instance-group handle
/// outlives a particular stack frame but must keep talking to the same dynamic-link universe.
#[derive(Clone)]
pub(in crate::state::linker) struct LinkerShared {
    /// Global module tables, buses, … — see [`LinkerState`].
    linker_state: Arc<RwLock<LinkerState>>,
    /// [`TopologyCoordinator`] embedded with this linker — guards topology-changing phases.
    ///
    /// At most **one** active [`TopologyToken`](TopologyToken) may exist cluster-wide while any
    /// topology mutation sequence is underway.
    topology_coordinator: TopologyCoordinator,
    /// Set during [`LinkerShared::synchronize_link_operation`] so syscall paths / cooperative writers
    /// can enter [`LinkerShared::do_pending_link_operations_internal`].
    dl_operation_pending: Arc<AtomicBool>,
    cancellation: LinkerCancellation,
}

impl LinkerShared {
    /// Wraps freshly constructed [`LinkerState`] for the owning process/module tree (initially only
    /// the main [`super::super::Linker::new`] path).
    pub(in crate::state::linker) fn new(
        linker_state: LinkerState,
        cancellation: LinkerCancellation,
    ) -> Self {
        Self {
            linker_state: Arc::new(RwLock::new(linker_state)),
            topology_coordinator: TopologyCoordinator::new(),
            dl_operation_pending: Arc::new(AtomicBool::new(false)),
            cancellation,
        }
    }

    pub(in crate::state::linker) fn check_active(&self, env: &WasiEnv) -> Result<(), LinkError> {
        self.cancellation.check(env)
    }

    pub(in crate::state::linker) fn cancellation(&self) -> &LinkerCancellation {
        &self.cancellation
    }

    /// Panics unless both DL buses have exactly one receiver — validates main-group bootstrap before
    /// exclusive writes (see [`Self::bootstrap_exclusive_write_then`]).
    fn assert_exactly_one_dl_bus_subscriber(ls: &LinkerState) {
        let op_rx = ls.send_pending_operation.rx_count();
        let barrier_rx = ls.send_pending_operation_barrier.rx_count();
        if op_rx != 1 || barrier_rx != 1 {
            panic!(
                "wasix linker bootstrap invariant violated: expected exactly one DL bus subscriber \
                 on each sender (pending_operation rx={op_rx}, barrier rx={barrier_rx}); \
                 `LinkerShared::bootstrap_exclusive_write_then` must only run during main \
                 `Linker::new` finalization before additional instance groups attach receivers"
            );
        }
    }

    /// Exclusive [`LinkerState`] write for main linker bootstrap only.
    ///
    /// # Safety
    ///
    /// Must run only while exactly one instance group has subscribed to both DL buses (verified
    /// after the lock is taken — mismatch panics in release builds). Caller must respect instance-group /
    /// linker lock ordering used in [`super::super::Linker::new`].
    pub(in crate::state::linker) unsafe fn bootstrap_exclusive_write_then<R>(
        &self,
        f: impl FnOnce(&mut LinkerState) -> R,
    ) -> R {
        let mut guard = self.linker_state.write().unwrap();
        Self::assert_exactly_one_dl_bus_subscriber(&guard);
        f(&mut guard)
    }

    /// Non-blocking `try_write` on [`LinkerState`].
    ///
    /// Used sparingly where blocking would recurse into the linker (stub paths, best-effort
    /// resolution). Prefer [`Self::write_linker_state`] for normal cooperative writes.
    pub(in crate::state::linker) fn try_write_linker_state(
        &self,
    ) -> Result<
        RwLockWriteGuard<'_, LinkerState>,
        std::sync::TryLockError<RwLockWriteGuard<'_, LinkerState>>,
    > {
        self.linker_state.try_write()
    }

    /// Non-blocking `try_read` on [`LinkerState`].
    pub(in crate::state::linker) fn try_read_linker_state(
        &self,
    ) -> Result<
        RwLockReadGuard<'_, LinkerState>,
        std::sync::TryLockError<RwLockReadGuard<'_, LinkerState>>,
    > {
        self.linker_state.try_read()
    }

    /// Locks [`LinkerState`] for write using repeated `try_write` plus cooperative draining of
    /// pending dynamic-link replay and [`LinkerStateWriteBackoff`].
    ///
    /// Prefer this over naked [`RwLock::write`] / blocking `write()` from instance-group linker
    /// paths: another OS thread might hold the write lock while follower groups rendezvous at a DL
    /// barrier waiting for **this** thread to run [`Self::do_pending_link_operations_internal`].
    pub(in crate::state::linker) fn write_linker_state(
        &self,
        group_state: &mut InstanceGroupState,
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    ) -> Result<RwLockWriteGuard<'_, LinkerState>, LinkError> {
        let mut linker_write_backoff = LinkerStateWriteBackoff::new();
        loop {
            self.check_active(ctx.data())?;
            match self.linker_state.try_write() {
                Ok(guard) => {
                    self.check_active(ctx.data())?;
                    return Ok(guard);
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    linker_write_backoff.backoff();
                    let env = ctx.as_ref();
                    let mut store = ctx.as_store_mut();
                    self.do_pending_link_operations_internal(group_state, &mut store, &env)?;
                }
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err(self
                        .cancellation
                        .abort(wasmer_wasix_types::wasi::Errno::Noexec.into()));
                }
            }
        }
    }

    /// [`TopologyCoordinator::try_acquire`] loop with [`LinkerStateWriteBackoff`] plus cooperative drains
    /// of [`Self::do_pending_link_operations_internal`].
    ///
    /// **Lock ordering**: topology must be leased **before** taking [`LinkerState`] for write paths that
    /// change replicated topology (spawn prepare, guarded loads, [`super::super::Linker::resolve_export`],
    /// etc.).
    ///
    /// `prepare_for_instance_group` is the motivating case — the parent attaches no new subscribers until
    /// the child finalizes while still holding this token handed across threads.
    pub(in crate::state::linker) fn acquire_topology_token(
        &self,
        group_state: &mut InstanceGroupState,
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<WasiEnv>,
    ) -> Result<TopologyToken, LinkError> {
        let mut backoff = LinkerStateWriteBackoff::new();
        loop {
            self.check_active(env.as_ref(store))?;
            if let Some(t) = self.topology_coordinator.try_acquire() {
                self.check_active(env.as_ref(store))?;
                return Ok(t);
            }
            backoff.backoff();
            self.do_pending_link_operations_internal(group_state, store, env)?;
        }
    }

    /// Cancellation-aware [`RwLock`] write once a [`TopologyToken`] is already held (spawn finalization —
    /// e.g. [`super::super::Linker::create_instance_group`]).
    ///
    /// Returns `(token, guard)` — drop the **`guard`** before **`token`** to avoid extending the write
    /// critical section beyond topology decisions.
    pub(in crate::state::linker) fn write_linker_state_holding_topology(
        &self,
        topology: TopologyToken,
        env: &WasiEnv,
    ) -> Result<(TopologyToken, RwLockWriteGuard<'_, LinkerState>), LinkError> {
        let mut backoff = LinkerStateWriteBackoff::new();
        loop {
            self.check_active(env)?;
            match self.linker_state.try_write() {
                Ok(guard) => {
                    self.check_active(env)?;
                    return Ok((topology, guard));
                }
                Err(std::sync::TryLockError::WouldBlock) => backoff.backoff(),
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err(self
                        .cancellation
                        .abort(wasmer_wasix_types::wasi::Errno::Noexec.into()));
                }
            }
        }
    }

    fn read_linker_state(
        &self,
        env: &WasiEnv,
    ) -> Result<RwLockReadGuard<'_, LinkerState>, LinkError> {
        let mut backoff = LinkerStateWriteBackoff::new();
        loop {
            self.check_active(env)?;
            match self.linker_state.try_read() {
                Ok(guard) => {
                    self.check_active(env)?;
                    return Ok(guard);
                }
                Err(std::sync::TryLockError::WouldBlock) => backoff.backoff(),
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err(self
                        .cancellation
                        .abort(wasmer_wasix_types::wasi::Errno::Noexec.into()));
                }
            }
        }
    }

    /// Acquires topology (see [`Self::acquire_topology_token`]), then takes a cancellation-aware write lock via
    /// [`Self::write_linker_state_holding_topology`].
    ///
    /// Use this for paths that mutate [`LinkerState`] under the topology coordinator’s single-writer
    /// umbrella when the lease was **not** already taken elsewhere.
    pub(in crate::state::linker) fn write_linker_state_with_topology(
        &self,
        group_state: &mut InstanceGroupState,
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    ) -> Result<(TopologyToken, RwLockWriteGuard<'_, LinkerState>), LinkError> {
        let env = ctx.as_ref();
        let mut store = ctx.as_store_mut();
        let token = self.acquire_topology_token(group_state, &mut store, &env)?;
        self.write_linker_state_holding_topology(token, ctx.data())
    }

    /// Broadcasts [`DlOperation`] `op` to every instance-group receiver then waits for replay.
    ///
    /// Contracts:
    ///
    /// * `topology` must already belong to **this** instigating flow and was leased **before**
    ///   exclusive access to buses / tables was acquired.
    /// * `linker_state_write_lock` guards bus broadcast invariants (`try_broadcast` must succeed).
    /// * Cancellation, failed replay, or bus misuse permanently aborts this linker;
    ///   partially replicated topology cannot safely be reconciled or reused.
    /// * Drops `topology` when done (`num_groups <= 1`) or after the follower completion barrier.
    pub(in crate::state::linker) fn synchronize_link_operation(
        &self,
        topology: TopologyToken,
        op: DlOperation,
        mut linker_state_write_lock: RwLockWriteGuard<'_, LinkerState>,
        group_state: &mut InstanceGroupState,
        env: &WasiEnv,
    ) -> Result<(), LinkError> {
        self.check_active(env)?;
        trace!(?op, "Synchronizing link operation");

        let num_groups = linker_state_write_lock.send_pending_operation.rx_count();

        if num_groups <= 1 {
            trace!("No other living instance groups, nothing to do");
            drop(linker_state_write_lock);
            drop(topology);
            return Ok(());
        }

        // Tokio barriers are not cancellation-safe by themselves. An aborted
        // wait permanently closes this linker, so neither epoch can be reused.
        // Arm before publishing anything: every early exit must wake peers.
        let replay = self.cancellation.guard();
        let barrier = Arc::new(Barrier::new(num_groups));
        // Single-flight barrier envelope (bus depth is one intentionally).
        if linker_state_write_lock
            .send_pending_operation_barrier
            .try_broadcast(barrier.clone())
            .is_err()
        {
            return Err(self
                .cancellation
                .abort(wasmer_wasix_types::wasi::Errno::Noexec.into()));
        }

        // Wake followers so syscall paths re-enter cooperative DL helpers promptly.
        self.dl_operation_pending.store(true, Ordering::SeqCst);

        trace!("Signalling wasix threads to wake up");
        for thread in env
            .process
            .all_threads()
            .into_iter()
            .filter(|tid| *tid != env.tid())
        {
            env.process
                .signal_thread(&thread, wasmer_wasix_types::wasi::Signal::Sigwakeup);
        }

        trace!(%num_groups, "Waiting at barrier");
        self.cancellation.wait(env, barrier.wait())?;

        trace!("All threads now processing dl op");

        // Everyone saw [`Self::dl_operation_pending_load`] and will drive `recv` paths.
        self.dl_operation_pending.store(false, Ordering::SeqCst);

        // Still under write lock: publish the replicated command before releasing exclusivity.
        if linker_state_write_lock
            .send_pending_operation
            .try_broadcast(op.clone())
            .is_err()
        {
            return Err(self
                .cancellation
                .abort(wasmer_wasix_types::wasi::Errno::Noexec.into()));
        }

        // Downgrade to shared read while followers apply (`apply_dl_operation`); no topology writer
        // should race between the barrier epochs.
        trace!("Unlocking linker state");
        drop(linker_state_write_lock);
        let linker_state_read_lock = self.read_linker_state(env)?;

        // Drain local bus copies — frees mailbox capacity before the follower epoch completes.
        self.cancellation
            .recv(env, &mut group_state.recv_pending_operation_barrier)?;
        self.cancellation
            .recv(env, &mut group_state.recv_pending_operation)?;

        // Second rendezvous guarantees everyone finished before another writer can preempt read-only
        // application (see linker `sync` module discussion).
        trace!("Waiting for other threads to finish processing the dl op");
        self.cancellation.wait(env, barrier.wait())?;

        drop(linker_state_read_lock);
        drop(topology);
        replay.complete();

        trace!("Synchronization complete");
        Ok(())
    }

    /// Peek at the cooperative-DL handshake flag `dl_operation_pending` with arbitrary memory
    /// ordering.
    ///
    /// Prefer [`Ordering::SeqCst`] (`fast = false` in callers) whenever another thread waking from
    /// `Sigwakeup` must reliably observe transitions; relaxed loads are intentionally lossy —
    /// safe only when callers will retry promptly on their own syscall boundaries.
    pub(in crate::state::linker) fn dl_operation_pending_load(&self, ordering: Ordering) -> bool {
        self.dl_operation_pending.load(ordering)
    }

    /// Follow half of [`Self::synchronize_link_operation`] — participates in barriers, consumes the broadcast
    /// [`DlOperation`], and applies `op` to `group_state` under [`LinkerState`] read access.
    ///
    /// Intended for callers that already skipped the idle fast path (cheap load of
    /// `dl_operation_pending`) yet still need deterministic rendezvous semantics.
    ///
    /// Missing senders or failed replay abort the entire linker; they cannot be
    /// treated as a recoverable guest errno after some stores have been changed.
    pub(in crate::state::linker) fn do_pending_link_operations_internal(
        &self,
        group_state: &mut InstanceGroupState,
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<WasiEnv>,
    ) -> Result<(), LinkError> {
        self.check_active(env.as_ref(store))?;
        if !self.dl_operation_pending.load(Ordering::SeqCst) {
            return Ok(());
        }

        trace!("Pending link operation discovered, will process");

        let replay = self.cancellation.guard();
        let barrier = self.cancellation.recv(
            env.as_ref(store),
            &mut group_state.recv_pending_operation_barrier,
        )?;
        self.cancellation.wait(env.as_ref(store), barrier.wait())?;

        trace!("Past the barrier, now processing operation");

        // Barrier epoch complete — instigator downgraded writer→reader earlier, so follower reads OK.
        let op = self
            .cancellation
            .recv(env.as_ref(store), &mut group_state.recv_pending_operation)?;
        let linker_state = self.read_linker_state(env.as_ref(store))?;

        if let Err(error) = group_state.apply_dl_operation(linker_state.deref(), op, store, env) {
            tracing::warn!(
                ?error,
                "Replicated link operation failed; aborting all groups"
            );
            return Err(self.cancellation.abort(
                error
                    .termination_code()
                    .unwrap_or_else(|| wasmer_wasix_types::wasi::Errno::Noexec.into()),
            ));
        }

        trace!("Operation applied, now waiting at second barrier");

        // Rendezvous again so nobody leaves while others still mutate stores / tables concurrently.
        self.cancellation.wait(env.as_ref(store), barrier.wait())?;
        drop(linker_state);

        replay.complete();

        trace!("Pending link operation applied successfully");

        Ok(())
    }
}
