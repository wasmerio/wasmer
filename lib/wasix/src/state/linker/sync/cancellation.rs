//! A cancelled replay cannot be resumed: some groups may already have changed
//! their stores/tables. Keep cancellation sticky for the entire linker, not
//! merely for the current rendezvous.

use std::future::Future;

use tokio::sync::watch;
use virtual_mio::block_on;
use wasmer_wasix_types::wasi::{Errno, ExitCode};

use crate::WasiEnv;

use super::super::LinkError;
use super::LinkerStateWriteBackoff;

#[cfg(all(test, feature = "sys-thread", not(target_arch = "wasm32")))]
mod tests;

#[derive(Clone)]
pub(in crate::state::linker) struct LinkerCancellation {
    aborted: watch::Sender<Option<ExitCode>>,
}

impl LinkerCancellation {
    pub(in crate::state::linker) fn new() -> Self {
        Self {
            aborted: watch::channel(None).0,
        }
    }

    pub(super) fn abort(&self, code: ExitCode) -> LinkError {
        // The first failure wins, including when several participants notice
        // process termination at the same time. Do not hold the watch borrow
        // across waking subscribers.
        self.aborted.send_if_modified(|aborted| {
            if aborted.is_some() {
                false
            } else {
                *aborted = Some(code);
                true
            }
        });
        LinkError::SynchronizationAborted(self.aborted.borrow().unwrap())
    }

    /// Initialization can trap with Exit before process/thread completion is
    /// published. Its caller must publish this abort while it still owns the
    /// topology and linker write guards: partially installed modules must never
    /// become visible to a healthy peer through the already-loaded fast path.
    /// Ordinary link errors retain their existing recoverable behavior.
    pub(in crate::state::linker) fn abort_on_exit(&self, error: LinkError) -> LinkError {
        match error.termination_code() {
            Some(code) => self.abort(code),
            None => error,
        }
    }

    pub(in crate::state::linker) fn check(&self, env: &WasiEnv) -> Result<(), LinkError> {
        if let Some(code) = *self.aborted.borrow() {
            return Err(LinkError::SynchronizationAborted(code));
        }
        if let Some(code) = env.should_exit() {
            return Err(self.abort(code));
        }
        Ok(())
    }

    pub(in crate::state::linker) fn wait<T>(
        &self,
        env: &WasiEnv,
        work: impl Future<Output = T>,
    ) -> Result<T, LinkError> {
        self.check(env)?;
        let mut aborted = self.aborted.subscribe();
        let result = block_on(async {
            tokio::select! {
                biased;
                // Publish inside the winning future, before select drops the
                // other futures (not in its branch body). A barrier waiter may
                // only be dropped once every peer can observe the sticky abort.
                error = async { self.abort(env.wait_for_exit().await) } => Err(error),
                code = async {
                    loop {
                        if let Some(code) = *aborted.borrow_and_update() {
                            break code;
                        }
                        // `self` retains a sender for the whole wait.
                        aborted.changed().await.unwrap();
                    }
                } => Err(LinkError::SynchronizationAborted(code)),
                result = work => Ok(result),
            }
        })?;
        self.check(env)?;
        Ok(result)
    }

    pub(super) fn recv<T: Clone + Sync>(
        &self,
        env: &WasiEnv,
        receiver: &mut bus::BusReader<T>,
    ) -> Result<T, LinkError> {
        let mut backoff = LinkerStateWriteBackoff::new();
        loop {
            self.check(env)?;
            match receiver.try_recv() {
                Ok(value) => return Ok(value),
                Err(std::sync::mpsc::TryRecvError::Empty) => backoff.backoff(),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(self.abort(Errno::Noexec.into()));
                }
            }
        }
    }

    pub(super) fn guard(&self) -> ReplayGuard<'_> {
        ReplayGuard {
            cancellation: self,
            completed: false,
        }
    }
}

/// Never leave peers parked if the leader unwinds or fails between epochs.
pub(super) struct ReplayGuard<'a> {
    cancellation: &'a LinkerCancellation,
    completed: bool,
}

impl ReplayGuard<'_> {
    pub(super) fn complete(mut self) {
        self.completed = true;
    }
}

impl Drop for ReplayGuard<'_> {
    fn drop(&mut self) {
        if !self.completed {
            let _ = self.cancellation.abort(Errno::Noexec.into());
        }
    }
}
