//! Yama (security/yama/yama_lsm.c): the machine-wide `kernel.yama.ptrace_scope`, which limits
//! who may attach to whom with ptrace, and the exceptions processes declare with
//! `prctl(PR_SET_PTRACER)`.
//!
//! The scope starts at 1 (`YAMA_SCOPE_RELATIONAL`), Yama's default: a process may attach only
//! to its descendants, to processes that named it (or one of its ancestors) as their tracer, or
//! with `CAP_SYS_PTRACE` over the target's user namespace. Processes are named by their
//! kernel pid, which is never reused.

use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

use wasixv2_abi::Errno;

use crate::sync::MutexExt;

/// `YAMA_SCOPE_DISABLED`: classic ptrace permissions.
pub(crate) const SCOPE_DISABLED: u32 = 0;
/// `YAMA_SCOPE_RELATIONAL`: only descendants, or declared exceptions.
pub(crate) const SCOPE_RELATIONAL: u32 = 1;
/// `YAMA_SCOPE_CAPABILITY`: only with `CAP_SYS_PTRACE`.
pub(crate) const SCOPE_CAPABILITY: u32 = 2;
/// `YAMA_SCOPE_NO_ATTACH`: no attaching at all, and the scope stays.
pub(crate) const SCOPE_NO_ATTACH: u32 = 3;

/// Who a process allows to trace it (`struct ptrace_relation`): `None` for anyone
/// (`PR_SET_PTRACER_ANY`).
type Tracer = Option<u32>;

pub(crate) struct Yama {
    scope: AtomicU32,
    /// `ptracer_relations`: by tracee thread group.
    relations: Mutex<HashMap<u32, Tracer>>,
}

impl Default for Yama {
    fn default() -> Self {
        Self {
            scope: AtomicU32::new(SCOPE_RELATIONAL),
            relations: Mutex::default(),
        }
    }
}

impl Yama {
    /// `ptrace_scope`.
    pub(crate) fn scope(&self) -> u32 {
        self.scope.load(Ordering::Relaxed)
    }

    /// `yama_dointvec_minmax`'s write, once the caller's `CAP_SYS_PTRACE` was checked: 0 to 3,
    /// and once 3 was written, only 3 ("lock the max value if it ever gets set").
    pub(crate) fn set_scope(&self, scope: i32) -> Result<(), Errno> {
        let min = match self.scope() {
            SCOPE_NO_ATTACH => SCOPE_NO_ATTACH,
            _ => SCOPE_DISABLED,
        };
        let scope = u32::try_from(scope).map_err(|_| Errno::EINVAL)?;
        if !(min..=SCOPE_NO_ATTACH).contains(&scope) {
            return Err(Errno::EINVAL);
        }
        self.scope.store(scope, Ordering::Relaxed);
        Ok(())
    }

    /// `yama_ptracer_add`: `tracee` allows `tracer` (anyone for `None`), replacing what it
    /// allowed before.
    pub(crate) fn add(&self, tracer: Tracer, tracee: u32) {
        self.relations.lock_or_recover().insert(tracee, tracer);
    }

    /// `yama_ptracer_del(NULL, tracee)`.
    pub(crate) fn del(&self, tracee: u32) {
        self.relations.lock_or_recover().remove(&tracee);
    }

    /// The exception `tracee` declared, if any: `Some(None)` for anyone.
    pub(crate) fn exception(&self, tracee: u32) -> Option<Tracer> {
        self.relations.lock_or_recover().get(&tracee).copied()
    }

    /// `yama_task_free`: the process `pid` is gone, as tracee and as tracer.
    pub(crate) fn task_free(&self, pid: u32) {
        self.relations
            .lock_or_recover()
            .retain(|&tracee, tracer| tracee != pid && *tracer != Some(pid));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scope_takes_0_to_3_and_locks_at_3() {
        let yama = Yama::default();
        assert_eq!(yama.scope(), SCOPE_RELATIONAL);
        assert_eq!(yama.set_scope(4), Err(Errno::EINVAL));
        assert_eq!(yama.set_scope(-1), Err(Errno::EINVAL));
        yama.set_scope(0).unwrap();
        assert_eq!(yama.scope(), SCOPE_DISABLED);
        yama.set_scope(3).unwrap();
        assert_eq!(yama.set_scope(2), Err(Errno::EINVAL));
        yama.set_scope(3).unwrap();
        assert_eq!(yama.scope(), SCOPE_NO_ATTACH);
    }

    #[test]
    fn a_relation_goes_with_either_process() {
        let yama = Yama::default();
        yama.add(Some(7), 9);
        yama.add(None, 10);
        assert_eq!(yama.exception(9), Some(Some(7)));
        assert_eq!(yama.exception(10), Some(None));
        yama.task_free(7);
        assert_eq!(yama.exception(9), None);
        yama.task_free(10);
        assert_eq!(yama.exception(10), None);
    }
}
