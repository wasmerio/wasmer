use std::fmt::Display;

use super::*;

/// Various triggers that will cause the runtime to take snapshot
/// of the WASM state and store it in the snapshot file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SnapshotTrigger {
    /// Triggered when all the threads in the process goes idle
    Idle,
    /// Triggered when a listen syscall is invoked on a socket for the first time
    FirstListen,
    /// Triggered on reading the environment variables for the first time
    FirstEnviron,
    /// Triggered when the process reads stdin for the first time
    FirstStdin,
    /// Issued on the first interrupt signal (Ctrl + C) the process receives, after that normal CTRL-C will apply.
    FirstSigint,
    /// Triggered periodically based on a interval (default 10 seconds) which can be specified using the `snapshot-interval` option
    PeriodicInterval,
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
    /// Bootstrapping process
    Bootstrap,
    /// Transaction
    Transaction,
    /// Explicitly requested by the guest module
    Explicit,
}

impl SnapshotTrigger {
    pub fn only_once(&self) -> bool {
        matches!(
            self,
            Self::FirstListen
                | Self::FirstEnviron
                | Self::FirstStdin
                | Self::FirstSigint
                // TODO: I don't think this should be an only_once trigger, but
                // repeatable triggers currently get stuck in a loop
                | Self::Explicit
        )
    }
}

pub const DEFAULT_SNAPSHOT_TRIGGERS: [SnapshotTrigger; 5] = [
    SnapshotTrigger::Idle,
    SnapshotTrigger::FirstEnviron,
    SnapshotTrigger::FirstListen,
    SnapshotTrigger::FirstStdin,
    SnapshotTrigger::Explicit,
];

impl FromStr for SnapshotTrigger {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let s = s.trim().to_lowercase();
        Ok(match s.as_str() {
            "idle" => Self::Idle,
            "first-listen" => Self::FirstListen,
            "first-stdin" => Self::FirstStdin,
            "first-environ" => Self::FirstEnviron,
            "first-intr" | "first-sigint" | "first-ctrlc" | "first-ctrl-c" => Self::FirstSigint,
            "periodic-interval" => Self::PeriodicInterval,
            "intr" | "sigint" | "ctrlc" | "ctrl-c" => Self::Sigint,
            "alarm" | "timer" | "sigalrm" => Self::Sigalrm,
            "sigtstp" | "ctrlz" | "ctrl-z" => Self::Sigtstp,
            "stop" | "sigstop" => Self::Sigstop,
            "non-deterministic-call" => Self::NonDeterministicCall,
            "bootstrap" => Self::Bootstrap,
            "transaction" => Self::Transaction,
            "explicit" => Self::Explicit,
            a => return Err(anyhow::format_err!("invalid or unknown trigger ({a})")),
        })
    }
}

impl Display for SnapshotTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Bootstrap => "bootstrap",
                Self::Explicit => "explicit",
                Self::FirstEnviron => "first-environ",
                Self::FirstListen => "first-listen",
                Self::FirstSigint => "first-sigint",
                Self::FirstStdin => "first-stdin",
                Self::Idle => "idle",
                Self::NonDeterministicCall => "non-deterministic-call",
                Self::PeriodicInterval => "periodic-interval",
                Self::Sigalrm => "sigalrm",
                Self::Sigint => "sigint",
                Self::Sigtstp => "sigtstp",
                Self::Sigstop => "sigstop",
                Self::Transaction => "transaction",
            }
        )
    }
}
