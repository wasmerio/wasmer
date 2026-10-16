//! The v1 import shims, one module per syscall family.

pub(crate) mod clock;
pub(crate) mod exit;
pub mod fs;
pub(crate) mod futex;
pub(crate) mod io;
pub mod proc;
pub(crate) mod random;
pub(crate) mod signal;
pub mod sock;
pub(crate) mod stack;
pub(crate) mod startup;
pub(crate) mod thread;
pub mod tty;
