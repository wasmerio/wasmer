//! Heaps to implement WebAssembly linear memories.

use cranelift_codegen::ir::{GlobalValue, MemoryType, Type};
use wasmer_types::entity::entity_impl;

/// An opaque reference to a [`HeapData`][crate::HeapData].
///
/// While the order is stable, it is arbitrary.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(
    feature = "enable-serde",
    derive(serde_derive::Serialize, serde_derive::Deserialize)
)]
pub struct Heap(u32);
entity_impl!(Heap, "heap");

/// A heap implementing a WebAssembly linear memory.
///
/// Code compiled from WebAssembly runs in a sandbox where it can't access all
/// process memory. Instead, it is given a small set of memory areas to work in,
/// and all accesses are bounds checked. `cranelift-wasm` models this through
/// the concept of *heaps*.
///
/// Heap addresses can be smaller than the native pointer size, for example
/// unsigned `i32` offsets on a 64-bit architecture.
///
/// A heap appears as three consecutive ranges of address space:
///
/// 1. The *mapped pages* are the accessible memory range in the heap. A heap
///    may have a minimum guaranteed size which means that some mapped pages are
///    always present.
///
/// 2. The *unmapped pages* is a possibly empty range of address space that may
///    be mapped in the future when the heap is grown. They are addressable but
///    not accessible.
///
/// 3. The *offset-guard pages* is a range of address space that is guaranteed
///    to always cause a trap when accessed. It is used to optimize bounds
///    checking for heap accesses with a shared base pointer. They are
///    addressable but not accessible.
///
/// The *heap bound* is the total size of the mapped and unmapped pages. This is
/// the bound that `heap_addr` checks against. Memory accesses inside the heap
/// bounds can trap if they hit an unmapped page (which is not accessible).
///
/// Two styles of heaps are supported, *static* and *dynamic*. They behave
/// differently when resized.
///
/// #### Static heaps
///
/// A *static heap* starts out with all the address space it will ever need, so
/// it never moves to a different address. At the base address is a number of
/// mapped pages corresponding to the heap's current size. Then follows a number
/// of unmapped pages where the heap can grow up to its maximum size. After the
/// unmapped pages follow the offset-guard pages which are also guaranteed to
/// generate a trap when accessed.
///
/// #### Dynamic heaps
///
/// A *dynamic heap* can be relocated to a different base address when it is
/// resized, and its bound can move dynamically. The offset-guard pages move
/// when the heap is resized. The bound of a dynamic heap is stored in a global
/// value.
#[derive(Clone, PartialEq, Hash)]
#[cfg_attr(
    feature = "enable-serde",
    derive(serde_derive::Serialize, serde_derive::Deserialize)
)]
pub struct HeapData {
    /// The address of the start of the heap's storage.
    pub base: GlobalValue,

    /// Guaranteed minimum heap size in bytes. Heap accesses before `min_size`
    /// don't need bounds checking.
    pub min_size: u64,

    /// The maximum heap size in bytes.
    ///
    /// Heap accesses larger than this will always trap.
    pub max_size: Option<u64>,

    /// The memory type for the pointed-to memory, if using proof-carrying code.
    pub memory_type: Option<MemoryType>,

    /// Size in bytes of the offset-guard pages following the heap.
    pub offset_guard_size: u64,

    /// Heap style, with additional style-specific info.
    pub style: HeapStyle,

    /// The index type for the heap.
    pub index_type: Type,

    /// The log2 of this memory's page size.
    pub page_size_log2: u8,
}

/// Style of heap including style-specific information.
#[derive(Clone, PartialEq, Hash)]
#[cfg_attr(
    feature = "enable-serde",
    derive(serde_derive::Serialize, serde_derive::Deserialize)
)]
pub enum HeapStyle {
    /// A dynamic heap can be relocated to a different base address when it is
    /// grown.
    Dynamic {
        /// Global value providing the current bound of the heap in bytes.
        bound_gv: GlobalValue,
    },

    /// A static heap has a fixed base address and a number of not-yet-allocated
    /// pages before the offset-guard pages.
    Static {
        /// Heap bound in bytes. The offset-guard pages are allocated after the
        /// bound.
        bound: u64,
    },
}
