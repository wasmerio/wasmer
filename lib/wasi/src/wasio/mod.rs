//! The WASIO extension.
//!
//! WASIO extends WASI to provide high-performance, fully asynchronous I/O operations.
//! 
//! ## Execution model
//! 
//! WASIO is based on an event loop model, where the application enters the event loop to wait for events and gets notified when
//! any of them happens.
//! 
//! WASIO handles the copying of socket buffers itself and only notifies the Wasm application when the data is ready in its
//! linear memory. This is different from poll-based methods like `epoll`/`kqueue` with both advantages
//! and disadvantages; but considering the overhead incurred by the Wasm VM we decide that a push-based model would work better.
//! Compatibility with poll-based methods may be added later.
//! 
//! On a low level, a typical WASIO application would work like following:
//! 
//! 1. Schedule at least one asynchronous event.
//! 2. Invoke `wasio_wait` to wait for an arriving event.
//! 3. Decode and handle the event. Usually a callback function is associated with the event, in which case it should be called.
//! 4. Go to step 2 to wait for the next event.
//! 
//! On a high level, WASIO is designed to be compatible with the Rust `async/await` mechanism with little overhead. 

pub mod executor;
pub mod executor_impl;
pub mod socket;
pub mod types;

pub use self::executor::Executor;
pub use self::executor_impl::dummy::DummyExecutor;
pub use self::executor_impl::tokio::TokioExecutor;
