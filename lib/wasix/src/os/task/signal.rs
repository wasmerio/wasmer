use std::{sync::Arc, time::Duration};

use wasmer_wasix_types::types::Signal;

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

pub type DynSignalHandlerAbi = dyn SignalHandlerAbi + Send + Sync + 'static;

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

pub fn default_signal_handler() -> Arc<DynSignalHandlerAbi> {
    #[derive(Debug)]
    struct DefaultHandler {}
    impl SignalHandlerAbi for DefaultHandler {
        fn signal(&self, signal: u8) -> Result<(), SignalDeliveryError> {
            if let Ok(signal) = TryInto::<Signal>::try_into(signal) {
                match signal {
                    Signal::Sigkill
                    | Signal::Sigterm
                    | Signal::Sigabrt
                    | Signal::Sigquit
                    | Signal::Sigint
                    | Signal::Sigstop => {
                        tracing::debug!("handling terminate signal");
                        std::process::exit(1);
                    }
                    signal => tracing::info!("unhandled signal - {:?}", signal),
                }
            } else {
                tracing::info!("unknown signal - {}", signal)
            }
            Ok(())
        }
    }
    Arc::new(DefaultHandler {})
}
