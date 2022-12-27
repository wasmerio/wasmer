mod fd_mmap;
mod memories;

pub use self::memories::{initialize_memory_with_data, VMMemory, VMOwnedMemory, VMSharedMemory};
