#[doc(hidden)]
pub const SAFE_STATIC_HEAP_SIZE: usize = 1 << 32; // 4 GiB
#[doc(hidden)]
pub const SAFE_STATIC_GUARD_SIZE: usize = 1 << 31; // 2 GiB

mod shared;
mod unshared;

pub use self::shared::SharedStaticMemory;
pub use self::unshared::StaticMemory;
