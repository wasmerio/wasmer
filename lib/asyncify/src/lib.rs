//! Drive WebAssembly modules instrumented with [asyncify] from Wasmer.
//!
//! An asyncified module can unwind its own stack back to the host and later
//! be rewound to where it left off. A host function drives this:
//!
//! 1. It calls [`Asyncify::start_unwind`], leaves an [`OnCalledHandler`] on
//!    its function environment and returns.
//! 2. The guest unwinds back to the host call that entered it. When that call
//!    was made through [`AsyncifyFunction`], the handler runs there, with the
//!    stack fully unwound.
//! 3. The handler calls [`Asyncify::stop_unwind`] and decides what happens
//!    next through [`OnCalledAction`]: re-enter the function after
//!    [`Asyncify::start_rewind`], finish the call, or trap.
//! 4. On re-entry the guest rewinds into the host function, which sees
//!    [`AsyncifyState::Rewinding`], calls [`Asyncify::stop_rewind`] and
//!    returns normally.
//!
//! The environment type tells the call where the handler is through
//! [`AsyncifyEnv`].
//!
//! [asyncify]: https://emscripten.org/docs/porting/asyncify.html

#![deny(missing_docs)]

mod call;
mod exports;

pub use call::{
    AsyncifyCallWrapper, AsyncifyEnv, AsyncifyFunction, OnCalledAction, OnCalledHandler, on_called,
};
pub use exports::{Asyncify, AsyncifyData, AsyncifyError, AsyncifyState};
