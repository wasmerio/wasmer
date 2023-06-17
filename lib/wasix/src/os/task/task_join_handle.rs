use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use wasmer_wasix_types::wasi::{Errno, ExitCode};

use crate::WasiRuntimeError;

#[derive(Clone, Debug)]
pub enum TaskStatus {
    Pending,
    Running,
    Finished(Result<ExitCode, Arc<WasiRuntimeError>>),
}

impl TaskStatus {
    /// Returns `true` if the task status is [`Pending`].
    ///
    /// [`Pending`]: TaskStatus::Pending
    #[must_use]
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending)
    }

    /// Returns `true` if the task status is [`Running`].
    ///
    /// [`Running`]: TaskStatus::Running
    #[must_use]
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }

    pub fn into_finished(self) -> Option<Result<ExitCode, Arc<WasiRuntimeError>>> {
        match self {
            Self::Finished(res) => Some(res),
            _ => None,
        }
    }

    /// Returns `true` if the task status is [`Finished`].
    ///
    /// [`Finished`]: TaskStatus::Finished
    #[must_use]
    pub fn is_finished(&self) -> bool {
        matches!(self, Self::Finished(..))
    }
}

#[derive(thiserror::Error, Debug)]
#[error("Task already terminated")]
pub struct TaskTerminatedError;

pub trait VirtualTaskHandle: std::fmt::Debug + Send + Sync + 'static {
    fn status(&self) -> TaskStatus;

    /// Polls to check if the process is ready yet to receive commands
    fn poll_ready(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TaskTerminatedError>>;

    fn poll_finished(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ExitCode, Arc<WasiRuntimeError>>>;
}

/// A handle that allows awaiting the termination of a task, and retrieving its exit code.
#[derive(Debug)]
pub struct OwnedTaskStatus {
    watch_tx: tokio::sync::watch::Sender<TaskStatus>,
    // Even through unused, without this receive there is a race condition
    // where the previously sent values are lost.
    #[allow(dead_code)]
    watch_rx: tokio::sync::watch::Receiver<TaskStatus>,
}

impl OwnedTaskStatus {
    pub fn new(status: TaskStatus) -> Self {
        let (tx, rx) = tokio::sync::watch::channel(status);
        Self {
            watch_tx: tx,
            watch_rx: rx,
        }
    }

    pub fn new_finished_with_code(code: ExitCode) -> Self {
        Self::new(TaskStatus::Finished(Ok(code)))
    }

    /// Marks the task as finished.
    pub fn set_running(&self) {
        self.watch_tx.send_modify(|value| {
            // Only set to running if task was pending, otherwise the transition would be invalid.
            if value.is_pending() {
                *value = TaskStatus::Running;
            }
        })
    }

    /// Marks the task as finished.
    pub(crate) fn set_finished(&self, res: Result<ExitCode, Arc<WasiRuntimeError>>) {
        let inner = match res {
            Ok(code) => Ok(code),
            Err(err) => {
                if let Some(code) = err.as_exit_code() {
                    Ok(code)
                } else {
                    Err(err)
                }
            }
        };
        self.watch_tx.send_modify(move |old| {
            if !old.is_finished() {
                *old = TaskStatus::Finished(inner);
            }
        });
    }

    pub fn status(&self) -> TaskStatus {
        self.watch_tx.borrow().clone()
    }

    pub async fn await_termination(&self) -> Result<ExitCode, Arc<WasiRuntimeError>> {
        let mut receiver = self.watch_tx.subscribe();
        loop {
            let status = receiver.borrow_and_update().clone();
            match status {
                TaskStatus::Pending | TaskStatus::Running => {}
                TaskStatus::Finished(res) => {
                    return res;
                }
            }
            // NOTE: unwrap() is fine, because &self always holds on to the sender.
            receiver.changed().await.unwrap();
        }
    }

    pub fn handle(&self) -> TaskJoinHandle {
        TaskJoinHandle {
            watch: self.watch_tx.subscribe(),
        }
    }
}

impl Default for OwnedTaskStatus {
    fn default() -> Self {
        Self::new(TaskStatus::Pending)
    }
}

/// A handle that allows awaiting the termination of a task, and retrieving its exit code.
#[derive(Clone, Debug)]
pub struct TaskJoinHandle {
    watch: tokio::sync::watch::Receiver<TaskStatus>,
}

impl TaskJoinHandle {
    /// Retrieve the current status.
    pub fn status(&self) -> TaskStatus {
        self.watch.borrow().clone()
    }

    /// Wait until the task finishes.
    pub async fn wait_finished(&mut self) -> Result<ExitCode, Arc<WasiRuntimeError>> {
        loop {
            let status = self.watch.borrow_and_update().clone();
            match status {
                TaskStatus::Pending | TaskStatus::Running => {}
                TaskStatus::Finished(res) => {
                    return res;
                }
            }
            if self.watch.changed().await.is_err() {
                return Ok(Errno::Noent.into());
            }
        }
    }
}
