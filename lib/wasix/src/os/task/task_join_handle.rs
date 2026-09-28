use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use wasmer_wasix_types::wasi::{Errno, ExitCode};

use crate::WasiRuntimeError;

use super::signal::{DynSignalHandlerAbi, default_signal_handler};

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
    // The signal handler that can be invoked for this owned task
    signal_handler: Arc<DynSignalHandlerAbi>,

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
            signal_handler: default_signal_handler(),
            watch_tx: tx,
            watch_rx: rx,
        }
    }

    /// Sets the signal handler used for this owned task
    pub fn set_signal_handler(&mut self, handler: Arc<DynSignalHandlerAbi>) {
        self.signal_handler = handler;
    }

    /// Attaches a signal handler
    pub fn with_signal_handler(mut self, handler: Arc<DynSignalHandlerAbi>) -> Self {
        self.set_signal_handler(handler);
        self
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

    pub async fn await_termination_anyhow(&self) -> anyhow::Result<ExitCode> {
        Ok(self.await_termination().await?)
    }

    pub fn handle(&self) -> TaskJoinHandle {
        TaskJoinHandle {
            signal_handler: self.signal_handler.clone(),
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
    #[allow(unused)]
    signal_handler: Arc<DynSignalHandlerAbi>,
    watch: tokio::sync::watch::Receiver<TaskStatus>,
}

impl TaskJoinHandle {
    /// Retrieve the current status.
    pub fn status(&self) -> TaskStatus {
        self.watch.borrow().clone()
    }

    #[cfg(feature = "ctrlc")]
    pub fn install_ctrlc_handler(&self) {
        use wasmer::FromToNativeWasmType;
        use wasmer_wasix_types::wasi::Signal;

        let signal_handler = self.signal_handler.clone();

        tokio::spawn(async move {
            // Loop sending ctrl-c presses as signals to the signal handler
            while tokio::signal::ctrl_c().await.is_ok() {
                if let Err(err) = signal_handler.signal(Signal::Sigint.to_native() as u8) {
                    tracing::error!("failed to process signal - {}", err);
                    std::process::exit(1);
                }
            }
        });
    }

    /// Relay OS-directed `SIGTERM`/`SIGHUP` into the guest's signal machinery.
    ///
    /// The relay is conditional because nothing else enforces a default action:
    /// `Thread::signal` only queues, and `process_signals_and_exit` ignores a
    /// queued `SIGTERM` outright when the guest never registered a handler. So
    /// an unconditional relay would replace "installing a handler kills the
    /// host" with "SIGTERM can never kill the host" -- a worse bug. Until
    /// `callback_signal` has run, the handler installed here is removed again
    /// and the signal re-raised, which leaves the shell-visible status exactly
    /// as it was before this function existed.
    ///
    /// The gate is per-process rather than per-signal, because the guest's
    /// disposition table lives in guest memory and `callback_signal` only tells
    /// the host that `__wasm_signal` exists. wasix-libc registers it during
    /// startup for every `crt1-command` module (libc-top-half/musl/src/signal/
    /// sigaction.c:486-489, called from libc-bottom-half/crt/crt1-command.c:42),
    /// so on such a module the re-raise branch is never reached -- measured: an
    /// external `SIGTERM` to a probe that installed no disposition exits `27`
    /// where the unrelayed runtime exited `143`, i.e. `128+SIGTERM`, the shell's
    /// own notation for the default action killing the process. `27` is
    /// `Self::Intr` (lib/wasi-types/src/wasi/bindings.rs:2820), so the status is
    /// the WASI signal-exit code; which layer converts an unhandled relayed
    /// signal into it is not settled by that measurement, and it is not musl's
    /// printed default action -- the string `terminate_handler` writes never
    /// appears in the guest log of such a run. What is settled is that once the
    /// callback is registered the guest owns the disposition, and a module that
    /// installs one for `SIGTERM` gets a handler call instead of either status
    /// (verified end to end against a `pgrust` postmaster).
    #[cfg(all(unix, feature = "ctrlc"))]
    pub fn install_os_signal_relay(
        &self,
        handler_registered: Arc<dyn Fn() -> bool + Send + Sync + 'static>,
    ) {
        use tokio::signal::unix::{signal, SignalKind};
        use wasmer::FromToNativeWasmType;
        use wasmer_wasix_types::wasi::Signal;

        for (kind, sig, signum) in [
            (SignalKind::terminate(), Signal::Sigterm, libc::SIGTERM),
            (SignalKind::hangup(), Signal::Sighup, libc::SIGHUP),
        ] {
            let signal_handler = self.signal_handler.clone();
            let handler_registered = handler_registered.clone();
            let num = sig.to_native() as u8;
            tokio::spawn(async move {
                // `SignalKind` is only a descriptor; subscribing is the fallible step,
                // and it has to happen inside the task that owns the stream.
                let mut stream = match signal(kind) {
                    Ok(stream) => stream,
                    Err(err) => {
                        tracing::error!("failed to install signal relay for {num}: {err}");
                        return;
                    }
                };
                while stream.recv().await.is_some() {
                    if !handler_registered() {
                        // SAFETY: SIG_DFL plus re-raise is the documented way to
                        // terminate with the default disposition from inside a
                        // handler; no guest or runtime state is touched.
                        unsafe {
                            libc::signal(signum, libc::SIG_DFL);
                            libc::raise(signum);
                        }
                        tracing::debug!(signum, "no guest signal handler, default action re-raised");
                        return;
                    }
                    match signal_handler.signal(num) {
                        Ok(()) => tracing::debug!(signum, "relayed host signal to guest"),
                        Err(err) => tracing::error!("failed to process signal - {err}"),
                    }
                }
            });
        }
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
