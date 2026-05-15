use std::{fmt::Display, path::Path};

use wasmer::{DetachedMemory, Instance, Memory, Table, TableType};

use super::{sync::LinkerShared, sync::TopologyToken};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleHandle(pub(super) u32);

impl From<ModuleHandle> for u32 {
    fn from(handle: ModuleHandle) -> Self {
        handle.0
    }
}

impl From<u32> for ModuleHandle {
    fn from(handle: u32) -> Self {
        ModuleHandle(handle)
    }
}

impl Display for ModuleHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub enum ResolvedExport {
    Function { func_ptr: u64 },

    // Contains the offset of the global in memory, with memory_base/tls_base accounted for
    // See: https://github.com/WebAssembly/tool-conventions/blob/main/DynamicLinking.md#exports
    Global { data_ptr: u64 },
}

pub struct LinkedMainModule {
    pub instance: Instance,
    pub memory: Memory,
    pub indirect_function_table: Table,
    pub stack_low: u64,
    pub stack_high: u64,
}

pub struct PreparedInstanceGroupData {
    pub(super) linker_shared: LinkerShared,
    /// Held from `prepare_for_instance_group`; child uses this with
    /// [`LinkerShared::write_linker_state_blocking_holding_topology`]
    /// on [`Self::linker_shared`].
    pub(super) topology_token: TopologyToken,

    // Data read from the parent context
    pub(super) memory: DetachedMemory,
    pub(super) indirect_function_table_type: TableType,
    pub(super) expected_table_length: u32,
}

pub enum DlModuleSpec<'a> {
    FileSystem {
        module_spec: &'a Path,
        ld_library_path: &'a [&'a Path],
    },
    Memory {
        module_name: &'a str,
        bytes: &'a [u8],
    },
}

impl std::fmt::Debug for DlModuleSpec<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileSystem { module_spec, .. } => f
                .debug_struct("FileSystem")
                .field("module_spec", module_spec)
                .finish(),
            Self::Memory { module_name, .. } => f
                .debug_struct("Memory")
                .field("module_name", module_name)
                .finish(),
        }
    }
}
