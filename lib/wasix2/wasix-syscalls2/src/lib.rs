//! Wasmer host functions for wasix2.
//!
//! [`Syscalls`] has one method per imported function; implement the ones a host supports and
//! register them with [`imports`]. The trait, the [`host`] functions and [`imports`] are
//! generated from the WIT definitions by `wasix-codegen`; see the header of `src/generated.rs`.

mod generated;

pub use generated::*;

/// Placeholder host state: every syscall fails with `ENOSYS`.
#[derive(Debug, Default)]
pub struct Wasix2Env {
    /// The guest's linear memory, once instantiated.
    ///
    /// TODO: replace with the real process/thread state when syscalls are implemented.
    pub memory: Option<wasmer::Memory>,
}

impl Wasix2Env {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Syscalls for Wasix2Env {}
