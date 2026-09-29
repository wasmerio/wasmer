//! Single-writer gate for WASIX linker topology changes.
//!
//! While a [`TopologyToken`] is held, no other topology-changing operation may begin.
//! The token may be moved to another thread (it is [`Send`]).
//!
//! Use [`TopologyCoordinator::try_acquire`] once per cooperative retry loop alongside
//! [`LinkerStateWriteBackoff`](super::LinkerStateWriteBackoff), analogous to the cooperative linker-state writers in this module (`write_linker_state`, `write_linker_state_with_topology`).

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

struct TopologyGateState {
    /// When `Some`, a [`TopologyToken`] owns the gate.
    active_token: Option<u64>,
}

struct TopologyCoordinatorInner {
    next_topology_token: AtomicU64,
    /// Test-only: counts successful [`TopologyCoordinator::try_acquire`] grants.
    #[cfg(test)]
    topology_generation: AtomicU64,
    gate: Mutex<TopologyGateState>,
}

/// Shared coordinator embedded in [`LinkerShared`](super::LinkerShared).
///
/// Call [`Self::try_acquire`] from an outer retry loop once pending DL ops have been
/// cooperated with; do not spin forever without backoff (see [`super::LinkerStateWriteBackoff`](super::LinkerStateWriteBackoff)).
#[derive(Clone)]
pub(super) struct TopologyCoordinator {
    inner: Arc<TopologyCoordinatorInner>,
}

impl TopologyCoordinator {
    pub(super) fn new() -> Self {
        Self {
            inner: Arc::new(TopologyCoordinatorInner {
                next_topology_token: AtomicU64::new(0),
                #[cfg(test)]
                topology_generation: AtomicU64::new(0),
                gate: Mutex::new(TopologyGateState { active_token: None }),
            }),
        }
    }

    /// Grants the topology lease if idle; otherwise [`None`] (caller should retry after cooperation + backoff).
    pub(super) fn try_acquire(&self) -> Option<TopologyToken> {
        let mut guard = self.inner.gate.lock().unwrap_or_else(|e| e.into_inner());
        if guard.active_token.is_some() {
            return None;
        }
        let token = self
            .inner
            .next_topology_token
            .fetch_add(1, Ordering::SeqCst);
        guard.active_token = Some(token);
        drop(guard);

        #[cfg(test)]
        self.inner
            .topology_generation
            .fetch_add(1, Ordering::SeqCst);

        Some(TopologyToken {
            inner: Arc::clone(&self.inner),
            token,
        })
    }

    #[cfg(test)]
    fn active_token_debug(&self) -> Option<u64> {
        let g = self.inner.gate.lock().unwrap_or_else(|e| e.into_inner());
        g.active_token
    }

    #[cfg(test)]
    fn topology_generation_debug(&self) -> u64 {
        self.inner.topology_generation.load(Ordering::SeqCst)
    }
}

/// RAII lease on linker topology serialization.
///
/// Dropping clears the coordinator so another [`TopologyCoordinator::try_acquire`] can succeed.
pub(crate) struct TopologyToken {
    inner: Arc<TopologyCoordinatorInner>,
    token: u64,
}

impl TopologyToken {
    /// Stable id for this lease (distinct per coordinator while they are handed out sequentially).
    #[cfg(test)]
    pub(crate) fn token_id_debug(&self) -> u64 {
        self.token
    }
}

impl Drop for TopologyToken {
    fn drop(&mut self) {
        let mut guard = self.inner.gate.lock().unwrap_or_else(|e| e.into_inner());
        debug_assert_eq!(guard.active_token, Some(self.token));
        guard.active_token = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn try_acquire_blocking(coord: &TopologyCoordinator) -> TopologyToken {
        loop {
            if let Some(t) = coord.try_acquire() {
                return t;
            }
            std::thread::yield_now();
        }
    }

    #[test]
    fn second_try_acquire_succeeds_only_after_drop() {
        let coord = TopologyCoordinator::new();
        let t1 = coord.try_acquire().expect("vacant coordinator");
        let (done_tx, done_rx) = mpsc::sync_channel::<()>(0);
        let coord2 = coord.clone();
        let th = std::thread::spawn(move || {
            let _blocked = try_acquire_blocking(&coord2);
            done_tx.send(()).unwrap();
        });
        std::thread::sleep(Duration::from_millis(20));
        assert!(done_rx.try_recv().is_err());
        drop(t1);
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        th.join().unwrap();
    }

    #[test]
    fn tokens_are_ordered_and_generation_advances() {
        let coord = TopologyCoordinator::new();
        assert_eq!(coord.topology_generation_debug(), 0);
        assert!(coord.active_token_debug().is_none());
        let a = coord.try_acquire().unwrap();
        assert_eq!(a.token_id_debug(), 0);
        assert_eq!(coord.topology_generation_debug(), 1);
        drop(a);
        let b = coord.try_acquire().unwrap();
        assert_eq!(b.token_id_debug(), 1);
        assert_eq!(coord.topology_generation_debug(), 2);
    }

    #[test]
    fn contended_try_acquire_resolves_quickly_after_drop() {
        let coord = Arc::new(TopologyCoordinator::new());
        let barrier = Arc::new(std::sync::Barrier::new(2));

        let c1 = Arc::clone(&coord);
        let b1 = Arc::clone(&barrier);
        let th = std::thread::spawn(move || {
            let _guard = try_acquire_blocking(&c1);
            b1.wait();
            std::thread::sleep(Duration::from_millis(30));
        });

        barrier.wait();
        let start = Instant::now();
        let _second = try_acquire_blocking(&coord);
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(200),
            "waited {:?}, expected uncontended-ish acquire after holder exit",
            elapsed
        );

        th.join().unwrap();
    }
}
