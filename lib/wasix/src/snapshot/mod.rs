mod capturer;
mod compactor;
#[cfg(feature = "snapshot")]
mod effector;
mod filter;
mod log_file;
mod unsupported;

pub use capturer::*;
pub use compactor::*;
#[cfg(feature = "snapshot")]
pub use effector::*;
pub use filter::*;
pub use log_file::*;
pub use unsupported::*;

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Various triggers that will cause the runtime to take snapshot
/// of the WASM state and store it in the snapshot file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SnapshotTrigger {
    /// Triggered when all the threads in the process goes idle
    Idle,
    /// Triggered when a listen syscall is invoked on a socket
    Listen,
    /// Triggered on reading the environment variables for the first time
    Environ,
    /// Triggered when the process reads stdin for the first time
    Stdin,
    /// Triggered periodically based on a timer (default 10 seconds) which can be specified using the `snapshot-timer` option
    Timer,
    /// Issued if the user sends an interrupt signal (Ctrl + C).
    Sigint,
    /// Alarm clock signal (used for timers)
    Sigalrm,
    /// The SIGTSTP signal is sent to a process by its controlling terminal to request it to stop temporarily. It is commonly initiated by the user pressing Ctrl-Z.
    Sigtstp,
    /// The SIGSTOP signal instructs the operating system to stop a process for later resumption.
    Sigstop,
    /// When a non-determinstic call is made
    NonDeterministicCall,
}

impl FromStr for SnapshotTrigger {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let s = s.trim().to_lowercase();
        Ok(match s.as_str() {
            "idle" => Self::Idle,
            "listen" => Self::Listen,
            "stdin" => Self::Stdin,
            "environ" => Self::Environ,
            "periodic" => Self::Timer,
            "intr" | "sigint" | "ctrlc" | "ctrl-c" => Self::Sigint,
            "alarm" | "timer" | "sigalrm" => Self::Sigalrm,
            "sigtstp" | "ctrlz" | "ctrl-z" => Self::Sigtstp,
            "stop" | "sigstop" => Self::Sigstop,
            "non-deterministic-call" => Self::NonDeterministicCall,
            a => return Err(anyhow::format_err!("invalid or unknown trigger ({a})")),
        })
    }
}