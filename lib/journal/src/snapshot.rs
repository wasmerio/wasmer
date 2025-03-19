use std::fmt::Display;

use wasmer_config::app::SnapshotTrigger as ConfigSnapshotTrigger;

use super::*;

/// Various triggers that will cause the runtime to take snapshot
/// of the WASM state and store it in the snapshot file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

// We're purposefully redirecting serialization-related functionality for SnapshotTrigger
// through the equivalent type in wasmer_config (ConfigSnapshotTrigger) to make sure the
// two types always stay in sync.
impl From<ConfigSnapshotTrigger> for SnapshotTrigger {
    fn from(value: ConfigSnapshotTrigger) -> Self {
        match value {
            ConfigSnapshotTrigger::Bootstrap => Self::Bootstrap,
            ConfigSnapshotTrigger::Explicit => Self::Explicit,
            ConfigSnapshotTrigger::FirstEnviron => Self::FirstEnviron,
            ConfigSnapshotTrigger::FirstListen => Self::FirstListen,
            ConfigSnapshotTrigger::FirstSigint => Self::FirstSigint,
            ConfigSnapshotTrigger::FirstStdin => Self::FirstStdin,
            ConfigSnapshotTrigger::Idle => Self::Idle,
            ConfigSnapshotTrigger::NonDeterministicCall => Self::NonDeterministicCall,
            ConfigSnapshotTrigger::PeriodicInterval => Self::PeriodicInterval,
            ConfigSnapshotTrigger::Sigalrm => Self::Sigalrm,
            ConfigSnapshotTrigger::Sigint => Self::Sigint,
            ConfigSnapshotTrigger::Sigstop => Self::Sigstop,
            ConfigSnapshotTrigger::Sigtstp => Self::Sigtstp,
            ConfigSnapshotTrigger::Transaction => Self::Transaction,
        }
    }
}

impl From<SnapshotTrigger> for ConfigSnapshotTrigger {
    fn from(value: SnapshotTrigger) -> Self {
        match value {
            SnapshotTrigger::Bootstrap => Self::Bootstrap,
            SnapshotTrigger::Explicit => Self::Explicit,
            SnapshotTrigger::FirstEnviron => Self::FirstEnviron,
            SnapshotTrigger::FirstListen => Self::FirstListen,
            SnapshotTrigger::FirstSigint => Self::FirstSigint,
            SnapshotTrigger::FirstStdin => Self::FirstStdin,
            SnapshotTrigger::Idle => Self::Idle,
            SnapshotTrigger::NonDeterministicCall => Self::NonDeterministicCall,
            SnapshotTrigger::PeriodicInterval => Self::PeriodicInterval,
            SnapshotTrigger::Sigalrm => Self::Sigalrm,
            SnapshotTrigger::Sigint => Self::Sigint,
            SnapshotTrigger::Sigstop => Self::Sigstop,
            SnapshotTrigger::Sigtstp => Self::Sigtstp,
            SnapshotTrigger::Transaction => Self::Transaction,
        }
    }
}

impl FromStr for SnapshotTrigger {
    type Err = <ConfigSnapshotTrigger as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        <ConfigSnapshotTrigger as FromStr>::from_str(s).map(Into::into)
    }
}

impl Display for SnapshotTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <ConfigSnapshotTrigger as Display>::fmt(&(*self).into(), f)
    }
}

impl Serialize for SnapshotTrigger {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ConfigSnapshotTrigger::from(*self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SnapshotTrigger {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        <ConfigSnapshotTrigger as Deserialize>::deserialize(deserializer).map(Into::into)
    }
}
