use crate::lib::std::sync::Arc;
use std::iter::FromIterator;
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "enable-borsh")]
use borsh::{BorshDeserialize, BorshSerialize};

use wasmer_types::entity::PrimaryMap;
use wasmer_types::{Features, MemoryIndex, TableIndex};
use wasmer_vm::{MemoryStyle, ModuleInfo, TableStyle};

/// The required info for compiling a module.
///
/// This differs from [`ModuleInfo`] because it have extra info only
/// possible after translation (such as the features used for compiling,
/// or the `MemoryStyle` and `TableStyle`).
#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
pub struct CompileModuleInfo {
    /// The features used for compiling the module
    pub features: Features,
    /// The module information
    pub module: Arc<ModuleInfo>,
    /// The memory styles used for compiling.
    ///
    /// The compiler will emit the most optimal code based
    /// on the memory style (static or dynamic) chosen.
    pub memory_styles: PrimaryMap<MemoryIndex, MemoryStyle>,
    /// The table plans used for compiling.
    pub table_styles: PrimaryMap<TableIndex, TableStyle>,
}

#[cfg(feature = "enable-borsh")]
impl BorshSerialize for CompileModuleInfo {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        BorshSerialize::serialize(&self.features, writer)?;
        BorshSerialize::serialize(&self.module.as_ref(), writer)?;
        BorshSerialize::serialize(&self.memory_styles.values().collect::<Vec<_>>(), writer)?;
        BorshSerialize::serialize(&self.table_styles.values().collect::<Vec<_>>(), writer)
    }
}

#[cfg(feature = "enable-borsh")]
impl BorshDeserialize for CompileModuleInfo {
    fn deserialize(buf: &mut &[u8]) -> std::io::Result<Self> {
        let features: Features = BorshDeserialize::deserialize(buf)?;
        let module: ModuleInfo = BorshDeserialize::deserialize(buf)?;
        let module = Arc::new(module);
        let memory_styles: Vec<MemoryStyle> = BorshDeserialize::deserialize(buf)?;
        let memory_styles: PrimaryMap<MemoryIndex, MemoryStyle> = PrimaryMap::from_iter(memory_styles);
        let table_styles: Vec<TableStyle> = BorshDeserialize::deserialize(buf)?;
        let table_styles: PrimaryMap<TableIndex, TableStyle> = PrimaryMap::from_iter(table_styles);
        Ok(Self { features, module, memory_styles, table_styles })
    }
}