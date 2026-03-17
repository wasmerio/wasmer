use core::ffi::c_void;

mod bridge;

pub use bridge::*;

pub type SnapiEnv = *mut SnapiEnvState;

#[repr(C)]
pub struct SnapiUnofficialHeapStatistics {
    pub total_heap_size: u64,
    pub total_heap_size_executable: u64,
    pub total_physical_size: u64,
    pub total_available_size: u64,
    pub used_heap_size: u64,
    pub heap_size_limit: u64,
    pub does_zap_garbage: u64,
    pub malloced_memory: u64,
    pub peak_malloced_memory: u64,
    pub number_of_native_contexts: u64,
    pub number_of_detached_contexts: u64,
    pub total_global_handles_size: u64,
    pub used_global_handles_size: u64,
    pub external_memory: u64,
}

#[repr(C)]
pub struct SnapiUnofficialHeapSpaceStatistics {
    pub space_name: [u8; 64],
    pub space_size: u64,
    pub space_used_size: u64,
    pub space_available_size: u64,
    pub physical_space_size: u64,
}

#[repr(C)]
pub struct SnapiUnofficialHeapCodeStatistics {
    pub code_and_metadata_size: u64,
    pub bytecode_and_metadata_size: u64,
    pub external_script_source_size: u64,
    pub cpu_profiler_metadata_size: u64,
}
