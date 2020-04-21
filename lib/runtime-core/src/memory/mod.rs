//! The memory module contains the implementation data structures and helper functions used to
//! manipulate and access wasm memory.
use crate::{
    error::{CreationError, GrowError},
    export::Export,
    import::IsExport,
    memory::dynamic::DYNAMIC_GUARD_SIZE,
    memory::static_::{SAFE_STATIC_GUARD_SIZE, SAFE_STATIC_HEAP_SIZE},
    types::{self, ValueType},
    units::Pages,
    vm,
};
use std::{cell::Cell, fmt, mem, sync::Arc};

use std::sync::Mutex as StdMutex;

pub use self::dynamic::DynamicMemory;
pub use self::static_::StaticMemory;
pub use self::view::{Atomically, MemoryView};

use parking_lot::Mutex;

mod dynamic;
pub mod ptr;
mod static_;
mod view;

#[derive(Clone)]
enum MemoryVariant {
    Unshared(UnsharedMemory),
    Shared(SharedMemory),
}

/// A shared or unshared wasm linear memory.
///
/// A `Memory` represents the memory used by a wasm instance.
#[derive(Clone)]
pub struct Memory {
    desc: types::MemoryType,
    variant: MemoryVariant,
}

impl Memory {
    /// Create a new `Memory` from a [`types::MemoryType`]
    ///
    /// [`types::MemoryType`]: struct.types::MemoryType.html
    ///
    /// Usage:
    ///
    /// ```
    /// # use wasmer_runtime_core::types;
    /// # use wasmer_runtime_core::memory::Memory;
    /// # use wasmer_runtime_core::error::Result;
    /// # use wasmer_runtime_core::units::Pages;
    /// fn create_memory() -> Result<()> {
    ///     let descriptor = types::MemoryType::new(Pages(10), None, false).unwrap();
    ///
    ///     let memory = Memory::new(descriptor)?;
    ///     Ok(())
    /// }
    /// ```
    pub fn new(desc: types::MemoryType) -> Result<Self, CreationError> {
        if let Some(max) = desc.maximum {
            if max < desc.minimum {
                return Err(CreationError::InvalidDescriptor(
                    "Max number of memory pages is less than the minimum number of pages"
                        .to_string(),
                ));
            }
        }

        if desc.shared && desc.maximum.is_none() {
            return Err(CreationError::InvalidDescriptor(
                "Max number of pages is required for shared memory".to_string(),
            ));
        }

        let variant = if !desc.shared {
            MemoryVariant::Unshared(UnsharedMemory::new(desc)?)
        } else {
            MemoryVariant::Shared(SharedMemory::new(desc)?)
        };

        Ok(Memory { desc, variant })
    }

    /// Return the [`types::MemoryType`] that this memory
    /// was created with.
    ///
    /// [`types::MemoryType`]: struct.types::MemoryType.html
    pub fn descriptor(&self) -> types::MemoryType {
        self.desc
    }

    /// Grow this memory by the specified number of pages.
    pub fn grow(&self, delta: Pages) -> Result<Pages, GrowError> {
        match &self.variant {
            MemoryVariant::Unshared(unshared_mem) => unshared_mem.grow(delta),
            MemoryVariant::Shared(shared_mem) => shared_mem.grow(delta),
        }
    }

    /// The size, in wasm pages, of this memory.
    pub fn size(&self) -> Pages {
        match &self.variant {
            MemoryVariant::Unshared(unshared_mem) => unshared_mem.size(),
            MemoryVariant::Shared(shared_mem) => shared_mem.size(),
        }
    }

    /// Return a "view" of the currently accessible memory. By
    /// default, the view is unsynchronized, using regular memory
    /// accesses. You can force a memory view to use atomic accesses
    /// by calling the [`atomically`] method.
    ///
    /// [`atomically`]: memory/struct.MemoryView.html#method.atomically
    ///
    /// # Notes:
    ///
    /// This method is safe (as in, it won't cause the host to crash or have UB),
    /// but it doesn't obey rust's rules involving data races, especially concurrent ones.
    /// Therefore, if this memory is shared between multiple threads, a single memory
    /// location can be mutated concurrently without synchronization.
    ///
    /// # Usage:
    ///
    /// ```
    /// # use wasmer_runtime_core::memory::{Memory, MemoryView};
    /// # use std::{cell::Cell, sync::atomic::Ordering};
    /// # fn view_memory(memory: Memory) {
    /// // Without synchronization.
    /// let view: MemoryView<u8> = memory.view();
    /// for byte in view[0x1000 .. 0x1010].iter().map(Cell::get) {
    ///     println!("byte: {}", byte);
    /// }
    ///
    /// // With synchronization.
    /// let atomic_view = view.atomically();
    /// for byte in atomic_view[0x1000 .. 0x1010].iter().map(|atom| atom.load(Ordering::SeqCst)) {
    ///     println!("byte: {}", byte);
    /// }
    /// # }
    /// ```
    pub fn view<T: ValueType>(&self) -> MemoryView<T> {
        let vm::LocalMemory { base, .. } = unsafe { *self.vm_local_memory() };

        let length = self.size().bytes().0 / mem::size_of::<T>();

        unsafe { MemoryView::new(base as _, length as u32) }
    }

    pub(crate) fn vm_local_memory(&self) -> *mut vm::LocalMemory {
        match &self.variant {
            MemoryVariant::Unshared(unshared_mem) => unshared_mem.vm_local_memory(),
            MemoryVariant::Shared(shared_mem) => shared_mem.vm_local_memory(),
        }
    }
}

impl IsExport for Memory {
    fn to_export(&self) -> Export {
        Export::Memory(self.clone())
    }
}

impl fmt::Debug for Memory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Memory")
            .field("desc", &self.desc)
            .field("size", &self.size())
            .finish()
    }
}

/// Legacy wrapper around [`BackingMemoryType`].
#[deprecated(note = "Please use `BackingMemoryType` instead.")]
pub type MemoryType = BackingMemoryType;

/// What the underlying memory should look like.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackingMemoryType {
    /// A dynamic memory.
    Dynamic,
    /// A static memory.
    Static,
    /// A shared static memory.
    SharedStatic,
}

impl BackingMemoryType {
    #[doc(hidden)]
    pub fn guard_size(self) -> u64 {
        match self {
            BackingMemoryType::Dynamic => DYNAMIC_GUARD_SIZE as u64,
            BackingMemoryType::Static | BackingMemoryType::SharedStatic => {
                SAFE_STATIC_GUARD_SIZE as u64
            }
        }
    }

    #[doc(hidden)]
    pub fn bounds(self) -> Option<u64> {
        match self {
            BackingMemoryType::Dynamic => None,
            BackingMemoryType::Static | BackingMemoryType::SharedStatic => {
                Some(SAFE_STATIC_HEAP_SIZE as u64)
            }
        }
    }
}

enum UnsharedMemoryStorage {
    Dynamic(Box<DynamicMemory>),
    Static(Box<StaticMemory>),
}

/// A reference to an unshared memory.
pub struct UnsharedMemory {
    internal: Arc<UnsharedMemoryInternal>,
}

struct UnsharedMemoryInternal {
    storage: StdMutex<UnsharedMemoryStorage>,
    local: Cell<vm::LocalMemory>,
}

// Manually implemented because UnsharedMemoryInternal uses `Cell` and is used in an Arc;
// this is safe because the lock for storage can be used to protect (seems like a weak reason: PLEASE REVIEW!)
unsafe impl Sync for UnsharedMemoryInternal {}

impl UnsharedMemory {
    /// Create a new `UnsharedMemory` from the given memory descriptor.
    pub fn new(desc: types::MemoryType) -> Result<Self, CreationError> {
        let mut local = vm::LocalMemory {
            base: std::ptr::null_mut(),
            bound: 0,
            memory: std::ptr::null_mut(),
        };

        let storage = match desc.memory_type() {
            BackingMemoryType::Dynamic => {
                UnsharedMemoryStorage::Dynamic(DynamicMemory::new(desc, &mut local)?)
            }
            BackingMemoryType::Static => {
                UnsharedMemoryStorage::Static(StaticMemory::new(desc, &mut local)?)
            }
            BackingMemoryType::SharedStatic => {
                return Err(CreationError::InvalidDescriptor(
                    "attempting to create shared unshared memory".to_string(),
                ));
            }
        };

        Ok(Self {
            internal: Arc::new(UnsharedMemoryInternal {
                storage: StdMutex::new(storage),
                local: Cell::new(local),
            }),
        })
    }

    /// Try to grow this memory by the given number of delta pages.
    pub fn grow(&self, delta: Pages) -> Result<Pages, GrowError> {
        let mut storage = self.internal.storage.lock().unwrap();

        let mut local = self.internal.local.get();

        let pages = match &mut *storage {
            UnsharedMemoryStorage::Dynamic(dynamic_memory) => {
                dynamic_memory.grow(delta, &mut local)
            }
            UnsharedMemoryStorage::Static(static_memory) => static_memory.grow(delta, &mut local),
        };

        self.internal.local.set(local);

        pages
    }

    /// Size of this memory in pages.
    pub fn size(&self) -> Pages {
        let storage = self.internal.storage.lock().unwrap();

        match &*storage {
            UnsharedMemoryStorage::Dynamic(ref dynamic_memory) => dynamic_memory.size(),
            UnsharedMemoryStorage::Static(ref static_memory) => static_memory.size(),
        }
    }

    pub(crate) fn vm_local_memory(&self) -> *mut vm::LocalMemory {
        self.internal.local.as_ptr()
    }
}

impl Clone for UnsharedMemory {
    fn clone(&self) -> Self {
        UnsharedMemory {
            internal: Arc::clone(&self.internal),
        }
    }
}

/// A reference to a shared memory.
pub struct SharedMemory {
    internal: Arc<SharedMemoryInternal>,
}

/// Data structure for a shared internal memory.
pub struct SharedMemoryInternal {
    memory: StdMutex<Box<StaticMemory>>,
    local: Cell<vm::LocalMemory>,
    lock: Mutex<()>,
}

// Manually implemented because SharedMemoryInternal uses `Cell` and is used in Arc;
// this is safe because of `lock`; accesing `local` without locking `lock` is not safe (Maybe we could put the lock on Local then?)
unsafe impl Sync for SharedMemoryInternal {}

impl SharedMemory {
    fn new(desc: types::MemoryType) -> Result<Self, CreationError> {
        let mut local = vm::LocalMemory {
            base: std::ptr::null_mut(),
            bound: 0,
            memory: std::ptr::null_mut(),
        };

        let memory = StaticMemory::new(desc, &mut local)?;

        Ok(Self {
            internal: Arc::new(SharedMemoryInternal {
                memory: StdMutex::new(memory),
                local: Cell::new(local),
                lock: Mutex::new(()),
            }),
        })
    }

    /// Try to grow this memory by the given number of delta pages.
    pub fn grow(&self, delta: Pages) -> Result<Pages, GrowError> {
        let _guard = self.internal.lock.lock();
        let mut local = self.internal.local.get();
        let mut memory = self.internal.memory.lock().unwrap();
        let pages = memory.grow(delta, &mut local);
        pages
    }

    /// Size of this memory in pages.
    pub fn size(&self) -> Pages {
        let _guard = self.internal.lock.lock();
        let memory = self.internal.memory.lock().unwrap();
        memory.size()
    }

    /// Gets a mutable pointer to the `LocalMemory`.
    // This function is scary, because the mutex is not locked here
    pub(crate) fn vm_local_memory(&self) -> *mut vm::LocalMemory {
        self.internal.local.as_ptr()
    }
}

impl Clone for SharedMemory {
    fn clone(&self) -> Self {
        SharedMemory {
            internal: Arc::clone(&self.internal),
        }
    }
}

#[cfg(test)]
mod memory_tests {

    use super::{types, Memory, Pages};

    #[test]
    fn test_initial_memory_size() {
        let memory_desc = types::MemoryType::new(Pages(10), Some(Pages(20)), false).unwrap();
        let unshared_memory = Memory::new(memory_desc).unwrap();
        assert_eq!(unshared_memory.size(), Pages(10));
    }

    #[test]
    fn test_invalid_descriptor_returns_error() {
        let memory_desc = types::MemoryType::new(Pages(10), None, true);
        assert!(
            memory_desc.is_err(),
            "Max number of pages is required for shared memory"
        )
    }
}
