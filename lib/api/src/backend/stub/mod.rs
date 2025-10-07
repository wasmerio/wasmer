//! Placeholder backend used when no real runtime backend features are enabled.

pub(crate) mod entities;
pub(crate) mod error;
pub(crate) mod vm;

#[inline(always)]
pub(crate) fn panic_stub<T>(msg: &str) -> T {
    panic!("stub backend does not provide runtime functionality: {msg}")
}
