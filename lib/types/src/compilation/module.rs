//! Types for modules.
use crate::entity::PrimaryMap;
use crate::{Features, MemoryIndex, MemoryStyle, ModuleInfo, TableIndex, TableStyle};
#[cfg(feature = "rkyv")]
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};

/// The required info for compiling a module.
///
/// This differs from [`ModuleInfo`] because it have extra info only
/// possible after translation (such as the features used for compiling,
/// or the `MemoryStyle` and `TableStyle`).
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[cfg_attr(
    feature = "enable-rkyv",
    derive(RkyvSerialize, RkyvDeserialize, Archive)
)]
#[derive(Debug, PartialEq, Eq)]
pub struct CompileModuleInfo {
    /// The features used for compiling the module
    pub features: Features,
    /// The module information
    pub module: ModuleInfo,
    /// The memory styles used for compiling.
    ///
    /// The compiler will emit the most optimal code based
    /// on the memory style (static or dynamic) chosen.
    pub memory_styles: PrimaryMap<MemoryIndex, MemoryStyle>,
    /// The table plans used for compiling.
    pub table_styles: PrimaryMap<TableIndex, TableStyle>,
}
