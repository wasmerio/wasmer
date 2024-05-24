#[cfg(feature = "enable-serde")]
use serde::*;

// pub mod wasix_http_client_v1;

#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "enable-serde", serde(rename_all = "snake_case"))]
pub enum ThreadStartType {
    MainThread,
    ThreadSpawn { start_ptr: u64 },
}

/// Represents the memory layout of the parts that the thread itself uses
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "enable-serde", serde(rename_all = "snake_case"))]
pub struct WasiMemoryLayout {
    /// This is the top part of the stack (stacks go backwards)
    pub stack_upper: u64,
    /// This is the bottom part of the stack (anything more below this is a stack overflow)
    pub stack_lower: u64,
    /// Piece of memory that is marked as none readable/writable so stack overflows cause an exception
    /// TODO: This field will need to be used to mark the guard memory as inaccessible
    #[allow(dead_code)]
    pub guard_size: u64,
    /// Total size of the stack
    pub stack_size: u64,
}
