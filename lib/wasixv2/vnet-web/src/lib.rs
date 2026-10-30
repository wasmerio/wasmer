//! Browser socket backend with bounded shared Rust queues and a worker-local I/O driver.
//!
//! The guest uses the existing vnet nonblocking traits. Poll [`Driver::poll`] on an independent
//! I/O worker; its [`Transport`] owns JS objects and never travels with guest socket handles.
//! Dropping a driver fails its session, including pending DNS callbacks. Reconnection creates
//! new flows only; old queued bytes are never replayed. See README.md for relay limitations.
mod backend;
pub mod codec;
mod driver;
pub mod transport;

pub use backend::{Config, WebBackend};
pub use driver::Driver;
pub use transport::{Capabilities, Classic, Command, Event, FrameTransport, Transport};
