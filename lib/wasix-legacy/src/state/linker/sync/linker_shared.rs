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
        Arc, Barrier, RwLock, RwLockReadGuard, RwLockWriteGuard,
        atomic::{AtomicBool, Ordering},
    },
};

use tracing::trace;
use wasmer::{AsStoreMut, FunctionEnv, FunctionEnvMut};

use crate::{WasiEnv, WasiProcess, WasiThreadId};

use super::super::{InstanceGroupState, LinkError, LinkerState};
use super::{
    DlOperation, LinkerStateWriteBackoff,
    topology_lock::{TopologyCoordinator, TopologyToken},
};

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
}

impl LinkerShared {
    /// Wraps freshly constructed [`LinkerState`] for the owning process/module tree (initially only
    /// the main [`super::super::Linker::new`] path).
    pub(in crate::state::linker) fn new(linker_state: LinkerState) -> Self {
        Self {
            linker_state: Arc::new(RwLock::new(linker_state)),
            topology_coordinator: TopologyCoordinator::new(),
            dl_operation_pending: Arc::new(AtomicBool::new(false)),
        }
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
            match self.linker_state.try_write() {
                Ok(guard) => return Ok(guard),
                Err(std::sync::TryLockError::WouldBlock) => {
                    linker_write_backoff.backoff();
                    let env = ctx.as_ref();
                    let mut store = ctx.as_store_mut();
                    self.do_pending_link_operations_internal(group_state, &mut store, &env)?;
                }
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    panic!("The linker state's lock is poisoned");
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
            if let Some(t) = self.topology_coordinator.try_acquire() {
                return Ok(t);
            }
            backoff.backoff();
            self.do_pending_link_operations_internal(group_state, store, env)?;
        }
    }

    /// Blocking [`RwLock`] write once a [`TopologyToken`] is already held (spawn finalization —
    /// e.g. [`super::super::Linker::create_instance_group`]).
    ///
    /// Returns `(token, guard)` — drop the **`guard`** before **`token`** to avoid extending the write
    /// critical section beyond topology decisions.
    pub(in crate::state::linker) fn write_linker_state_blocking_holding_topology(
        &self,
        topology: TopologyToken,
    ) -> (TopologyToken, RwLockWriteGuard<'_, LinkerState>) {
        let linker_state_write_guard = self.linker_state.write().unwrap();
        (topology, linker_state_write_guard)
    }

    /// Acquires topology (see [`Self::acquire_topology_token`]), then takes a blocking write lock via
    /// [`Self::write_linker_state_blocking_holding_topology`].
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
        Ok(self.write_linker_state_blocking_holding_topology(token))
    }

    /// Broadcasts [`DlOperation`] `op` to every instance-group receiver then waits for replay.
    ///
    /// Contracts:
    ///
    /// * `topology` must already belong to **this** instigating flow and was leased **before**
    ///   exclusive access to buses / tables was acquired.
    /// * `linker_state_write_lock` guards bus broadcast invariants (`try_broadcast` must succeed).
    /// * Recoverable semantic failures are surfaced by callers; panics here are always fatal —
    ///   bus capacity misuse or missed rendezvous implies we cannot reconcile groups.
    /// * Drops `topology` when done (`num_groups <= 1`) or after the follower completion barrier.
    pub(in crate::state::linker) fn synchronize_link_operation(
        &self,
        topology: TopologyToken,
        op: DlOperation,
        mut linker_state_write_lock: RwLockWriteGuard<'_, LinkerState>,
        group_state: &mut InstanceGroupState,
        wasi_process: &WasiProcess,
        self_thread_id: WasiThreadId,
    ) {
        trace!(?op, "Synchronizing link operation");

        let num_groups = linker_state_write_lock.send_pending_operation.rx_count();

        if num_groups <= 1 {
            trace!("No other living instance groups, nothing to do");
            drop(topology);
            return;
        }

        let barrier = Arc::new(Barrier::new(num_groups));
        // Single-flight barrier envelope (bus depth is one intentionally).
        if linker_state_write_lock
            .send_pending_operation_barrier
            .try_broadcast(barrier.clone())
            .is_err()
        {
            panic!("Internal error: more than one synchronized link operation active")
        }

        // Wake followers so syscall paths re-enter cooperative DL helpers promptly.
        self.dl_operation_pending.store(true, Ordering::SeqCst);

        trace!("Signalling wasix threads to wake up");
        for thread in wasi_process
            .all_threads()
            .into_iter()
            .filter(|tid| *tid != self_thread_id)
        {
            wasi_process.signal_thread(&thread, wasmer_wasix_types::wasi::Signal::Sigwakeup);
        }

        trace!(%num_groups, "Waiting at barrier");
        barrier.wait();

        trace!("All threads now processing dl op");

        // Everyone saw [`Self::dl_operation_pending_load`] and will drive `recv` paths.
        self.dl_operation_pending.store(false, Ordering::SeqCst);

        // Still under write lock: publish the replicated command before releasing exclusivity.
        if linker_state_write_lock
            .send_pending_operation
            .try_broadcast(op.clone())
            .is_err()
        {
            panic!("Internal error: more than one synchronized link operation active")
        }

        // Downgrade to shared read while followers apply (`apply_dl_operation`); no topology writer
        // should race between the barrier epochs.
        trace!("Unlocking linker state");
        drop(linker_state_write_lock);
        let linker_state_read_lock = self.linker_state.read().unwrap();

        // Drain local bus copies — frees mailbox capacity before the follower epoch completes.
        _ = group_state.recv_pending_operation_barrier.recv().unwrap();
        _ = group_state.recv_pending_operation.recv().unwrap();

        // Second rendezvous guarantees everyone finished before another writer can preempt read-only
        // application (see linker `sync` module discussion).
        trace!("Waiting for other threads to finish processing the dl op");
        barrier.wait();

        drop(linker_state_read_lock);
        drop(topology);

        trace!("Synchronization complete");
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
    /// # Panics
    ///
    /// Missing receivers / malformed bus state panic — those are irrecoverable and indicate we lost
    /// synchronization with subscribers.
    pub(in crate::state::linker) fn do_pending_link_operations_internal(
        &self,
        group_state: &mut InstanceGroupState,
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<WasiEnv>,
    ) -> Result<(), LinkError> {
        if !self.dl_operation_pending.load(Ordering::SeqCst) {
            return Ok(());
        }

        trace!("Pending link operation discovered, will process");

        let barrier = group_state.recv_pending_operation_barrier.recv().expect(
            "Failed to receive barrier while a DL operation was \
            in progress; this condition can't be recovered from",
        );
        barrier.wait();

        trace!("Past the barrier, now processing operation");

        // Barrier epoch complete — instigator downgraded writer→reader earlier, so follower reads OK.
        let op = group_state.recv_pending_operation.recv().unwrap();
        let linker_state = self.linker_state.read().unwrap();

        let result = group_state.apply_dl_operation(linker_state.deref(), op, store, env);

        trace!("Operation applied, now waiting at second barrier");

        // Rendezvous again so nobody leaves while others still mutate stores / tables concurrently.
        barrier.wait();
        drop(linker_state);

        trace!("Pending link operation applied successfully");

        result
    }
}
