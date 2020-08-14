//! The WASIO extension.
//!
//! WASIO extends WASI to provide high-performance, fully asynchronous I/O operations.

pub mod executor;
pub mod executor_impl;
pub mod socket;
pub mod types;

pub use self::executor::Executor;
pub use self::executor_impl::dummy::DummyExecutor;
pub use self::executor_impl::tokio::TokioExecutor;
