//! Dynamic-linking (`Dl*`) synchronization for the WASIX [`super::Linker`].
//!
//! Instance groups run on different OS threads and share one [`super::LinkerState`] behind an
//! [`std::sync::RwLock`]. Operations that change “who exists” or “what every group must agree on”
//! must coordinate with both that lock and a stop-the-world style broadcast of concrete mutations
//! ([`DlOperation`]). This module holds the primitives that make that safe.
//!
//! # Locks and responsibilities
//!
//! - **Instance-group mutex** ([`lock_instance_group_state!`], `Linker::instance_group_state`):
//!   Per-[`super::Linker`] handle to this thread’s [`InstanceGroupState`]. Many linker entry points
//!   take it first so they can call into group-local state and, when needed, run cooperative DL
//!   helpers with the right `Store` / [`FunctionEnv`].
//!
//! - **Topology lease** ([`LinkerShared`](LinkerShared) holds [`topology_lock::TopologyCoordinator`]
//!   privately): A single-writer gate for *topology-changing* work (new instance groups, module loads,
//!   export resolution that can allocate shared slots, etc.). The lease is acquired in a cooperative loop
//!   with backoff and pending-DL cooperation (see [`LinkerShared::acquire_topology_token`],
//!   [`LinkerShared::write_linker_state_with_topology`]). [`TopologyToken`] may move to another thread (spawn handoff).
//!
//! - **Shared linker state** (inside [`LinkerShared`], not exposed as a field): Global module
//!   table, symbol records, and the buses used to broadcast [`DlOperation`] and barriers. Writers
//!   must follow the cooperative patterns below—not raw lock calls.
//!
//! - **Pending-DL handshake** (`dl_operation_pending`, barriers, wakeup signals): While an
//!   instigator runs [`LinkerShared::synchronize_link_operation`], follower threads must enter
//!   [`Linker::do_pending_link_operations`] (or helpers) so everyone rendezvouses. That is why
//!   contended access to [`LinkerState`] cannot spin blindly.
//!
//! # Lock ordering (intended)
//!
//! When topology applies: **topology token first**, then lock [`LinkerState`] for write via the APIs
//! in this module—not the inverse. Never try to acquire a topology lease from inside code that
//! already holds [`LinkerState`] for write without a deliberate, reviewed plan (easy deadlock).
//!
//! # Why you must never lock `LinkerState` directly
//!
//! **Do not call `linker_state.read()`, `write()`, or `try_write()` on [`Linker`]’s [`RwLock`] from
//! normal instance-group linker paths.** Doing so skips the cooperative path and can deadlock the
//! whole process: another thread may hold the write lock while waiting at a DL barrier for *this*
//! thread to execute [`LinkerShared::do_pending_link_operations_internal`], which requires the same group
//! context and cannot run if this thread is stuck in a naive blocking `write()`.
//!
//! Use instead:
//!
//! - [`LinkerShared::write_linker_state`] — `try_write` loop + [`LinkerStateWriteBackoff`] + pending-DL draining.
//! - [`LinkerShared::write_linker_state_with_topology`] — topology lease + draining + blocking write when
//!   topology must be serialized before grabbing [`LinkerState`].
//! - [`LinkerShared::write_linker_state_blocking_holding_topology`] — blocking write only while already
//!   holding [`TopologyToken`], after topology was leased on another thread/step.
//!
//! Narrow exceptions (e.g. one-off bootstrap in [`super::Linker::new`] before other groups exist)
//! belong in tightly scoped code and should still avoid contending paths that overlap DL sync.

pub(super) mod topology_lock;

mod linker_shared;

pub(in crate::state::linker) use linker_shared::LinkerShared;
pub(crate) use topology_lock::TopologyToken;

use std::time::Duration;

use super::ModuleHandle;

/// Spin, then yield, then capped exponential sleep — for cooperative linker-state retries.
pub(super) struct LinkerStateWriteBackoff {
    /// Number of [`backoff`](Self::backoff) calls so far after a collision.
    step: u32,
}

impl LinkerStateWriteBackoff {
    const SPIN_PHASE_STEPS: u32 = 64;
    const YIELD_PHASE_STEPS: u32 = 24;
    const SLEEP_MICROS_INITIAL: u64 = 48;
    const SLEEP_MICROS_MAX: u64 = 10_000;
    const SLEEP_SHIFT_CAP: u32 = 8;

    pub(super) fn new() -> Self {
        Self { step: 0 }
    }

    /// Call once per failed `try_write` after cooperating on pending DL ops.
    pub(super) fn backoff(&mut self) {
        let step = self.step;
        self.step = self.step.saturating_add(1);

        if step < Self::SPIN_PHASE_STEPS {
            std::hint::spin_loop();
            return;
        }

        let after_spin = step - Self::SPIN_PHASE_STEPS;
        if after_spin < Self::YIELD_PHASE_STEPS {
            std::thread::yield_now();
            return;
        }

        let slept = step
            .saturating_sub(Self::SPIN_PHASE_STEPS)
            .saturating_sub(Self::YIELD_PHASE_STEPS);
        let shift = slept.min(Self::SLEEP_SHIFT_CAP);
        let micros = Self::SLEEP_MICROS_INITIAL
            .checked_shl(shift)
            .unwrap_or(Self::SLEEP_MICROS_MAX)
            .min(Self::SLEEP_MICROS_MAX);

        std::thread::sleep(Duration::from_micros(micros));
    }
}

macro_rules! lock_instance_group_state {
    ($guard:ident, $state:ident, $linker:expr, $err:expr) => {
        let mut $guard = $linker.instance_group_state.lock().unwrap();
        if $guard.is_none() {
            return Err($err);
        }
        let $state = $guard.deref_mut().as_mut().unwrap();
    };
}

pub(super) use lock_instance_group_state;

// Used to communicate the result of an operation that happened in one
// instance group to all others
#[derive(Debug, Clone)]
pub(super) enum DlOperation {
    LoadModules(Vec<ModuleHandle>),
    // Allocates slots in the function table
    AllocateFunctionTable { index: u32, size: u32 },
}
