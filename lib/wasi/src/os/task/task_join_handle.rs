use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use wasmer_wasi_types::wasi::{Errno, ExitCode};

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
    watch: tokio::sync::watch::Sender<TaskStatus>,
}

impl OwnedTaskStatus {
    pub fn new(status: TaskStatus) -> Self {
        Self {
            watch: tokio::sync::watch::channel(status).0,
        }
    }

    pub fn new_finished_with_code(code: ExitCode) -> Self {
        Self::new(TaskStatus::Finished(Ok(code)))
    }

    /// Marks the task as finished.
    pub fn set_running(&self) {
        self.watch.send_modify(|value| {
            // Only set to running if task was pending, otherwise the transition would be invalid.
            if value.is_pending() {
                *value = TaskStatus::Running;
            }
        })
    }

    /// Marks the task as finished.
    pub(super) fn set_finished(&self, res: Result<ExitCode, Arc<WasiRuntimeError>>) {
        // Don't overwrite a previous finished state.
        if self.status().is_finished() {
            return;
        }

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
        self.watch.send(TaskStatus::Finished(inner)).ok();
    }

    pub fn status(&self) -> TaskStatus {
        self.watch.borrow().clone()
    }

    pub async fn await_termination(&self) -> Result<ExitCode, Arc<WasiRuntimeError>> {
        let mut receiver = self.watch.subscribe();
        match &*receiver.borrow_and_update() {
            TaskStatus::Pending | TaskStatus::Running => {}
            TaskStatus::Finished(res) => {
                return res.clone();
            }
        }
        loop {
            // NOTE: unwrap() is fine, because &self always holds on to the sender.
            receiver.changed().await.unwrap();
            match &*receiver.borrow_and_update() {
                TaskStatus::Pending | TaskStatus::Running => {}
                TaskStatus::Finished(res) => {
                    return res.clone();
                }
            }
        }
    }

    pub fn handle(&self) -> TaskJoinHandle {
        TaskJoinHandle {
            watch: self.watch.subscribe(),
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
        match &*self.watch.borrow_and_update() {
            TaskStatus::Pending | TaskStatus::Running => {}
            TaskStatus::Finished(res) => {
                return res.clone();
            }
        }
        loop {
            if self.watch.changed().await.is_err() {
                return Ok(Errno::Noent as u32);
            }
            match &*self.watch.borrow_and_update() {
                TaskStatus::Pending | TaskStatus::Running => {}
                TaskStatus::Finished(res) => {
                    return res.clone();
                }
            }
        }
    }
}
