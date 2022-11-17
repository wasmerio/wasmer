use std::time::Duration;
use wasmer_wasi_types::types::Signal;

#[derive(Debug)]
pub struct WasiSignalInterval {
    /// Signal that will be raised
    pub signal: Signal,
    /// Time between the signals
    pub interval: Duration,
    /// Flag that indicates if the signal should repeat
    pub repeat: bool,
    /// Last time that a signal was triggered
    pub last_signal: u128,
}
