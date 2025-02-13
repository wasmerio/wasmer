//! Helper functions and structures for the translation.
use crate::entity::entity_impl;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};

/*
 * [todo] xdoardo (on 2024/10/09)
 *
 * While updating rkyv from 0.7.x to 0.8.x it became unfeasible to represent archived
 * version of some types as themselves (i.e. `rkyv(as = Self)`). This means that in cases
 * like this one we can't simply return a reference to the underlying value.
 *
 * Hopefully as 0.8.x matures a bit more we can return to `rkyv(as = Self)`, making all
 * of this faster and leaner. The missing bits for it are:
 *
 * 1. rkyv::rend::{u32_le, i32_le, u64_le, ..} should implement `serde::Serialize` and
 *    `serde::Deserialize`.
 *
 */

/// Index type of a function defined locally inside the WebAssembly module.
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Debug,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[rkyv(derive(Debug, Hash, PartialEq, Eq), compare(PartialOrd, PartialEq))]
pub struct LocalFunctionIndex(u32);
entity_impl!(LocalFunctionIndex);

/// Index type of a table defined locally inside the WebAssembly module.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct LocalTableIndex(u32);
entity_impl!(LocalTableIndex);

/// Index type of a memory defined locally inside the WebAssembly module.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct LocalMemoryIndex(u32);
entity_impl!(LocalMemoryIndex);

/// Index type of a tag defined locally inside the WebAssembly module.
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Debug,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[rkyv(derive(Debug), compare(PartialOrd, PartialEq))]
pub struct LocalTagIndex(u32);
entity_impl!(LocalTagIndex);

/// Index type of a global defined locally inside the WebAssembly module.
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Debug,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[rkyv(derive(Debug), compare(PartialOrd, PartialEq))]
pub struct LocalGlobalIndex(u32);
entity_impl!(LocalGlobalIndex);

/// Index type of a function (imported or local) inside the WebAssembly module.
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Debug,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[rkyv(
    derive(Debug, PartialOrd, Ord, PartialEq, Eq),
    compare(PartialOrd, PartialEq)
)]
pub struct FunctionIndex(u32);
entity_impl!(FunctionIndex);

/// Index type of a table (imported or local) inside the WebAssembly module.
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Debug,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[rkyv(
    derive(Debug, PartialOrd, Ord, PartialEq, Eq),
    compare(PartialOrd, PartialEq)
)]
pub struct TableIndex(u32);
entity_impl!(TableIndex);

/// Index type of an event inside the WebAssembly module.
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Debug,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[rkyv(
    derive(Debug, PartialOrd, Ord, PartialEq, Eq),
    compare(PartialOrd, PartialEq)
)]
pub struct TagIndex(u32);
entity_impl!(TagIndex);

/// Index type of a global variable (imported or local) inside the WebAssembly module.
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Debug,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[rkyv(derive(Debug), compare(PartialOrd, PartialEq))]
pub struct GlobalIndex(u32);
entity_impl!(GlobalIndex);

/// Index type of a linear memory (imported or local) inside the WebAssembly module.
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Debug,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[rkyv(derive(Debug), compare(PartialOrd, PartialEq))]
pub struct MemoryIndex(pub(crate) u32);
entity_impl!(MemoryIndex);

/// Index type of a signature (imported or local) inside the WebAssembly module.
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Debug,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[rkyv(derive(Debug), compare(PartialOrd, PartialEq))]
pub struct SignatureIndex(u32);
entity_impl!(SignatureIndex);

/// Index type of a passive data segment inside the WebAssembly module.
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Debug,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[rkyv(
    derive(Debug, PartialOrd, Ord, PartialEq, Eq),
    compare(PartialOrd, PartialEq)
)]
pub struct DataIndex(u32);
entity_impl!(DataIndex);

/// Index type of a passive element segment inside the WebAssembly module.
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Debug,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[rkyv(
    derive(Debug, PartialOrd, Ord, PartialEq, Eq),
    compare(PartialOrd, PartialEq)
)]
pub struct ElemIndex(u32);
entity_impl!(ElemIndex);

/// Index type of a custom section inside a WebAssembly module.
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Debug,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[rkyv(derive(Debug), compare(PartialOrd, PartialEq))]
pub struct CustomSectionIndex(u32);
entity_impl!(CustomSectionIndex);

/// An entity to export.
#[derive(
    Copy,
    Clone,
    Debug,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[rkyv(derive(Debug), compare(PartialOrd, PartialEq))]
#[repr(u8)]
pub enum ExportIndex {
    /// Function export.
    Function(FunctionIndex),
    /// Table export.
    Table(TableIndex),
    /// Memory export.
    Memory(MemoryIndex),
    /// An event definition.
    Tag(TagIndex),
    /// Global export.
    Global(GlobalIndex),
}

/// An entity to import.
#[derive(
    Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, RkyvSerialize, RkyvDeserialize, Archive,
)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[rkyv(derive(Debug), compare(PartialOrd, PartialEq))]
#[repr(u8)]
pub enum ImportIndex {
    /// Function import.
    Function(FunctionIndex),
    /// Table import.
    Table(TableIndex),
    /// Memory import.
    Memory(MemoryIndex),
    /// Tag import.
    Tag(TagIndex),
    /// Global import.
    Global(GlobalIndex),
}

/// WebAssembly event.
#[derive(
    Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, RkyvSerialize, RkyvDeserialize, Archive,
)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[rkyv(derive(Debug), compare(PartialOrd, PartialEq))]
pub struct Tag {
    /// The event signature type.
    pub ty: u32,
}
