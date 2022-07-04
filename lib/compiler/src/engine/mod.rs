//! The Wasmer Engine.

mod error;
mod resolver;
mod trap;
mod tunables;

#[cfg(feature = "translator")]
mod universal;

pub use self::error::{InstantiationError, LinkError};
pub use self::resolver::resolve_imports;
pub use self::trap::*;
pub use self::tunables::Tunables;

#[cfg(feature = "translator")]
pub use self::universal::*;
