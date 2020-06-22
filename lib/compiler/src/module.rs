#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use wasm_common::entity::PrimaryMap;
use wasm_common::{Features, MemoryIndex, TableIndex};
use wasmer_runtime::{MemoryPlan, ModuleInfo, TablePlan};

/// The required info for compiling a module.
///
/// This differs from [`ModuleInfo`] because it have extra info only
/// possible after translation (such as the features used for compiling,
/// or the `MemoryPlan` and `TablePlan`).
#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
pub struct CompileModuleInfo {
    /// The features used for compiling the module
    pub features: Features,
    /// The module information
    pub module: Arc<ModuleInfo>,
    /// The memory plans used for compiling.
    ///
    /// The compiler will emit the most optimal code based
    /// on the memory style (static or dynamic) chosen.
    pub memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
    /// The table plans used for compiling.
    pub table_plans: PrimaryMap<TableIndex, TablePlan>,
}
