use std::time::Duration;

use wasmer_wasi_types::types::Signal;

#[derive(thiserror::Error, Debug)]
#[error("Signal could not be delivered")]
pub struct SignalDeliveryError;

/// Signal handles...well...they process signals
pub trait SignalHandlerAbi
where
    Self: std::fmt::Debug,
{
    /// Processes a signal
    fn signal(&self, signal: u8) -> Result<(), SignalDeliveryError>;
}

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
