mod fd_mmap;
mod memories;

pub use self::memories::{VMMemory, VMOwnedMemory, VMSharedMemory, initialize_memory_with_data};
