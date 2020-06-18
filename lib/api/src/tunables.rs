use crate::memory::LinearMemory;
use crate::table::LinearTable;
use crate::{MemoryType, Pages, TableType};
use more_asserts::assert_ge;
use std::cmp::min;
use std::sync::Arc;
use target_lexicon::{OperatingSystem, PointerWidth, Triple, HOST};
use wasmer_compiler::Target;
use wasmer_engine::Tunables as BaseTunables;
use wasmer_runtime::MemoryError;
use wasmer_runtime::{Memory, MemoryPlan, MemoryStyle, Table, TablePlan, TableStyle};

/// Tunable parameters for WebAssembly compilation.
#[derive(Clone)]
pub struct Tunables {
    pub target: Target,

    /// For static heaps, the size in wasm pages of the heap protected by bounds checking.
    pub static_memory_bound: Pages,

    /// The size in bytes of the offset guard for static heaps.
    pub static_memory_offset_guard_size: u64,

    /// The size in bytes of the offset guard for dynamic heaps.
    pub dynamic_memory_offset_guard_size: u64,
}

impl Tunables {
    /// Get the `Tunables` for a specific Target
    pub fn for_target(target: Target) -> Self {
        let triple = target.triple();
        let pointer_width: PointerWidth = triple.pointer_width().unwrap();
        let (mut static_memory_bound, mut static_memory_offset_guard_size): (Pages, u64) =
            match pointer_width {
                PointerWidth::U16 => (0x400.into(), 0x1000),
                PointerWidth::U32 => (0x4000.into(), 0x1_0000),
                // Static Memory Bound:
                //   Allocating 4 GiB of address space let us avoid the
                //   need for explicit bounds checks.
                // Static Memory Guard size:
                //   Allocating 2 GiB of address space lets us translate wasm
                //   offsets into x86 offsets as aggressively as we can.
                PointerWidth::U64 => (0x1_0000.into(), 0x8000_0000),
            };

        // Allocate a small guard to optimize common cases but without
        // wasting too much memory.
        let dynamic_memory_offset_guard_size: u64 = 0x1_0000;

        if let OperatingSystem::Windows = triple.operating_system {
            // For now, use a smaller footprint on Windows so that we don't
            // outstrip the paging file.
            static_memory_bound = min(static_memory_bound, 0x100.into());
            static_memory_offset_guard_size = min(static_memory_offset_guard_size, 0x10000);
        }

        Self {
            static_memory_bound,
            static_memory_offset_guard_size,
            dynamic_memory_offset_guard_size,
            target,
        }
    }
}

impl BaseTunables for Tunables {
    /// Get the target for this Tunables
    fn target(&self) -> &Target {
        &self.target
    }

    /// Get a `MemoryPlan` for the provided `MemoryType`
    fn memory_plan(&self, memory: MemoryType) -> MemoryPlan {
        // A heap with a maximum that doesn't exceed the static memory bound specified by the
        // tunables make it static.
        //
        // If the module doesn't declare an explicit maximum treat it as 4GiB.
        let maximum = memory.maximum.unwrap_or_else(Pages::max_value);
        if maximum <= self.static_memory_bound {
            assert_ge!(self.static_memory_bound, memory.minimum);
            MemoryPlan {
                memory,
                style: MemoryStyle::Static {
                    bound: self.static_memory_bound,
                },
                offset_guard_size: self.static_memory_offset_guard_size,
            }
        } else {
            MemoryPlan {
                memory,
                style: MemoryStyle::Dynamic,
                offset_guard_size: self.dynamic_memory_offset_guard_size,
            }
        }
    }

    /// Get a `TablePlan` for the provided `TableType`
    fn table_plan(&self, table: TableType) -> TablePlan {
        TablePlan {
            table,
            style: TableStyle::CallerChecksSignature,
        }
    }

    /// Create a memory given a memory type
    fn create_memory(&self, plan: MemoryPlan) -> Result<Arc<dyn Memory>, MemoryError> {
        Ok(Arc::new(LinearMemory::new(&plan)?))
    }

    /// Create a memory given a memory type
    fn create_table(&self, plan: TablePlan) -> Result<Arc<dyn Table>, String> {
        Ok(Arc::new(LinearTable::new(&plan)?))
    }
}

impl Default for Tunables {
    fn default() -> Self {
        Self::for_target(Target::default())
    }
}
