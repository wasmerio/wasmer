//! The v1 import shims, one module per syscall family.

pub(crate) mod clock;
pub(crate) mod dir;
pub(crate) mod epoll;
pub(crate) mod event;
pub(crate) mod exit;
pub mod fs;
pub(crate) mod futex;
pub(crate) mod http;
pub(crate) mod io;
pub(crate) mod path;
pub(crate) mod poll;
pub(crate) mod port;
pub mod proc;
pub(crate) mod random;
pub(crate) mod resolve;
pub(crate) mod signal;
pub mod sock;
pub(crate) mod sockopt;
pub(crate) mod stack;
pub(crate) mod startup;
pub(crate) mod thread;
pub mod tty;
