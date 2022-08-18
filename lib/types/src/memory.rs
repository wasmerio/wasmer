use crate::{Pages, ValueType};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use core::ptr::NonNull;
use std::convert::{TryFrom, TryInto};
use std::iter::Sum;
use std::ops::{Add, AddAssign};

use super::MemoryType;
use super::MemoryError;

/// Implementation styles for WebAssembly linear memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, RkyvSerialize, RkyvDeserialize, Archive)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[archive(as = "Self")]
pub enum MemoryStyle {
    /// The actual memory can be resized and moved.
    Dynamic {
        /// Our chosen offset-guard size.
        ///
        /// It represents the size in bytes of extra guard pages after the end
        /// to optimize loads and stores with constant offsets.
        offset_guard_size: u64,
    },
    /// Address space is allocated up front.
    Static {
        /// The number of mapped and unmapped pages.
        bound: Pages,
        /// Our chosen offset-guard size.
        ///
        /// It represents the size in bytes of extra guard pages after the end
        /// to optimize loads and stores with constant offsets.
        offset_guard_size: u64,
    },
}

impl MemoryStyle {
    /// Returns the offset-guard size
    pub fn offset_guard_size(&self) -> u64 {
        match self {
            Self::Dynamic { offset_guard_size } => *offset_guard_size,
            Self::Static {
                offset_guard_size, ..
            } => *offset_guard_size,
        }
    }
}

/// Trait for the `Memory32` and `Memory64` marker types.
///
/// This allows code to be generic over 32-bit and 64-bit memories.
/// # Safety
/// Direct memory access is unsafe
pub unsafe trait MemorySize: Copy {
    /// Type used to represent an offset into a memory. This is `u32` or `u64`.
    type Offset: Default
        + std::fmt::Debug
        + std::fmt::Display
        + Eq
        + Ord
        + PartialEq<Self::Offset>
        + PartialOrd<Self::Offset>
        + Clone
        + Copy
        + Sync
        + Send
        + ValueType
        + Into<u64>
        + From<u32>
        + From<u16>
        + From<u8>
        + TryFrom<u64>
        + TryFrom<u32>
        + TryFrom<u16>
        + TryFrom<u8>
        + TryInto<usize>
        + TryInto<u64>
        + TryInto<u32>
        + TryInto<u16>
        + TryInto<u8>
        + TryFrom<usize>
        + Add<Self::Offset>
        + Sum<Self::Offset>
        + AddAssign<Self::Offset>
        + 'static;

    /// Type used to pass this value as an argument or return value for a Wasm function.
    type Native: super::NativeWasmType;

    /// Zero value used for `WasmPtr::is_null`.
    const ZERO: Self::Offset;

    /// One value used for counting.
    const ONE: Self::Offset;

    /// Convert an `Offset` to a `Native`.
    fn offset_to_native(offset: Self::Offset) -> Self::Native;

    /// Convert a `Native` to an `Offset`.
    fn native_to_offset(native: Self::Native) -> Self::Offset;
}

/// Marker trait for 32-bit memories.
#[derive(Clone, Copy)]
pub struct Memory32;
unsafe impl MemorySize for Memory32 {
    type Offset = u32;
    type Native = i32;
    const ZERO: Self::Offset = 0;
    const ONE: Self::Offset = 1;
    fn offset_to_native(offset: Self::Offset) -> Self::Native {
        offset as Self::Native
    }
    fn native_to_offset(native: Self::Native) -> Self::Offset {
        native as Self::Offset
    }
}

/// Marker trait for 64-bit memories.
#[derive(Clone, Copy)]
pub struct Memory64;
unsafe impl MemorySize for Memory64 {
    type Offset = u64;
    type Native = i64;
    const ZERO: Self::Offset = 0;
    const ONE: Self::Offset = 1;
    fn offset_to_native(offset: Self::Offset) -> Self::Native {
        offset as Self::Native
    }
    fn native_to_offset(native: Self::Native) -> Self::Offset {
        native as Self::Offset
    }
}

/// Represents different roles that a particular region of memory plays
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemoryRole
{
    /// The region is used for storing data (default)
    Data,
    /// The region is used as a stack
    Stack,
    /// The region is used to guard against memory access violations
    Guard,
    /// The region resides on another remote location (holds the reference number for that location)
    Remote(u64),
}

impl Default
for MemoryRole {
    fn default() -> Self {
        MemoryRole::Data
    }
}

/// Represents memory that is used by the WebAsssembly module
pub trait LinearMemory
where Self: std::fmt::Debug + Send
{
    /// Returns the type for this memory.
    fn ty(&self) -> MemoryType;

    /// Returns the size of hte memory in pages
    fn size(&self) -> Pages;

    /// Returns the memory style for this memory.
    fn style(&self) -> MemoryStyle;

    /// Grow memory by the specified amount of wasm pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of wasm pages.
    fn grow(&mut self, delta: Pages) -> Result<Pages, MemoryError>;

    /// Return a `VMMemoryDefinition` for exposing the memory to compiled wasm code.
    fn vmmemory(&self) -> NonNull<VMMemoryDefinition>;

    /// Attempts to clone this memory (if its clonable)
    fn try_clone(&self) -> Option<Box<dyn LinearMemory + 'static>>;

    /// Copies this memory to a new memory
    fn fork(&mut self) -> Result<Box<dyn LinearMemory + 'static>, MemoryError>;

    /// Marks a region of the memory for a particular role
    fn mark_region(&mut self, start: u64, end: u64, role: MemoryRole);

    /// Returns the role of a part of the memory
    fn region(&self, pointer: u64) -> MemoryRole;
}

/// The fields compiled code needs to access to utilize a WebAssembly linear
/// memory defined within the instance, namely the start address and the
/// size in bytes.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct VMMemoryDefinition {
    /// The start address which is always valid, even if the memory grows.
    pub base: *mut u8,

    /// The current logical size of this linear memory in bytes.
    pub current_length: usize,
}

/// # Safety
/// This data is safe to share between threads because it's plain data that
/// is the user's responsibility to synchronize.
unsafe impl Send for VMMemoryDefinition {}
/// # Safety
/// This data is safe to share between threads because it's plain data that
/// is the user's responsibility to synchronize. And it's `Copy` so there's
/// really no difference between passing it by reference or by value as far as
/// correctness in a multi-threaded context is concerned.
unsafe impl Sync for VMMemoryDefinition {}

#[cfg(test)]
mod test_vmmemory_definition {
    use super::VMMemoryDefinition;
    use crate::VMOffsets;
    use memoffset::offset_of;
    use std::mem::size_of;
    use crate::ModuleInfo;

    #[test]
    fn check_vmmemory_definition_offsets() {
        let module = ModuleInfo::new();
        let offsets = VMOffsets::new(size_of::<*mut u8>() as u8, &module);
        assert_eq!(
            size_of::<VMMemoryDefinition>(),
            usize::from(offsets.size_of_vmmemory_definition())
        );
        assert_eq!(
            offset_of!(VMMemoryDefinition, base),
            usize::from(offsets.vmmemory_definition_base())
        );
        assert_eq!(
            offset_of!(VMMemoryDefinition, current_length),
            usize::from(offsets.vmmemory_definition_current_length())
        );
    }
}
