#![deny(
    dead_code,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
#[cfg(windows)]
mod exception_handling;

#[cfg(windows)]
pub use self::exception_handling::*;
