use std::sync::Mutex;

use wasmer_wasi_types::wasi::ExitCode;

/// A handle that allows awaiting the termination of a task, and retrieving its exit code.
#[derive(Debug)]
pub struct TaskJoinHandle {
    exit_code: Mutex<Option<ExitCode>>,
    sender: tokio::sync::broadcast::Sender<()>,
}

impl TaskJoinHandle {
    pub fn new() -> Self {
        let (sender, _) = tokio::sync::broadcast::channel(1);
        Self {
            exit_code: Mutex::new(None),
            sender,
        }
    }

    /// Marks the task as finished.
    pub(super) fn terminate(&self, exit_code: u32) {
        let mut lock = self.exit_code.lock().unwrap();
        if lock.is_none() {
            *lock = Some(exit_code);
            std::mem::drop(lock);
            self.sender.send(()).ok();
        }
    }

    pub async fn await_termination(&self) -> Option<ExitCode> {
        // FIXME: why is this a loop? should not be necessary,
        // Should be redundant since the subscriber is created while holding the lock.
        loop {
            let mut rx = {
                let code_opt = self.exit_code.lock().unwrap();
                if code_opt.is_some() {
                    return code_opt.clone();
                }
                self.sender.subscribe()
            };
            if rx.recv().await.is_err() {
                return None;
            }
        }
    }

    /// Returns the exit code if the task has finished, and None otherwise.
    pub fn get_exit_code(&self) -> Option<ExitCode> {
        self.exit_code.lock().unwrap().clone()
    }
}
