//! Raw browser streams over the shared runtime heap. JS objects stay on the independent
//! coordinator/I/O worker; task and TTY service workers only poll Rust futures and park
//! through the executor's cross-worker Wakers. The UI uses messages to that coordinator.
//!
//! These are host transports, not Linux pipes: PIPE_BUF, SIGPIPE and descriptor poll policy
//! belong to the kernel. Terminal bytes go through the unchanged kernel line discipline
//! (`drivers/tty/n_tty.c:n_tty_receive_buf_common,n_tty_read`). In particular, send 0x03
//! for Ctrl-C and 0x04 for terminal EOF; closing stdin is persistent stream EOF/hangup.

mod queue;
pub use queue::{Sink, Source, StreamWake, channel};

use std::{io, sync::Arc};
use wasixv2_host::{Console, Stdio, WindowSize};

/// Coordinator endpoints. Consume stdout and stderr concurrently, including while waiting
/// for process exit; otherwise bounded output can intentionally block the guest.
pub struct Streams {
    pub stdin: Arc<Sink>,
    pub stdout: Arc<Source>,
    pub stderr: Arc<Source>,
}

/// Runtime owner of a run's streams. Use the returned Stdio in ProcessConfig. After all
/// tasks sharing the streams stop and `console_wait_until_sent` completes, call `finish`
/// and keep consuming both outputs
/// until EOF before publishing the final process result. Dropping this owner alone does
/// not revoke streams still held by a process; use `abort` for infrastructure failure.
pub struct Session {
    input: Arc<Source>,
    output: Arc<Sink>,
    error: Arc<Sink>,
}

impl Session {
    /// Each direction accepts at most `capacity` queued payload bytes. Zero is invalid.
    pub fn new(capacity: usize) -> io::Result<(Self, Stdio, Streams)> {
        let (input_tx, input_rx) = channel(capacity)?;
        let (output_tx, output_rx) = channel(capacity)?;
        let (error_tx, error_rx) = channel(capacity)?;
        let session = Self {
            input: Arc::new(input_rx),
            output: Arc::new(output_tx),
            error: Arc::new(error_tx),
        };
        let stdio = Stdio {
            stdin: session.input.clone(),
            stdout: session.output.clone(),
            stderr: session.error.clone(),
        };
        let streams = Streams {
            stdin: Arc::new(input_tx),
            stdout: Arc::new(output_rx),
            stderr: Arc::new(error_rx),
        };
        Ok((session, stdio, streams))
    }

    /// Use with MachineBuilder::console(console, [true, true, false]) to retain
    /// independent piped stderr. No key editing, echo or signal policy is implemented here.
    /// Forward resize/hangup on the coordinator via Signaller::console_resize/hangup;
    /// Linux drivers/tty/tty_io.c:tty_do_resize and do_tty_hangup own their semantics.
    pub fn console(&self, size: WindowSize) -> Console {
        Console {
            input: self.input.clone(),
            output: self.output.clone(),
            size,
        }
    }

    /// Seal outputs after their producers quiesce, then wait for both to drain. This is
    /// transport drain: bytes have reached the coordinator consumer, not necessarily the
    /// terminal display. The SDK must also await its Web Stream writes before returning.
    pub async fn finish(&self) -> io::Result<()> {
        self.input.cancel();
        self.output.close();
        self.error.close();
        let (out, err) =
            futures_util::future::join(self.output.drained(), self.error.drained()).await;
        out.and(err)
    }

    /// Infrastructure shutdown/cancel: discard queued input/output and wake every waiter.
    pub fn abort(&self) {
        self.input.cancel();
        self.output.abort();
        self.error.abort();
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
