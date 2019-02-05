use crate::{
    error::CreationError,
    export::Export,
    import::IsExport,
    memory::dynamic::DYNAMIC_GUARD_SIZE,
    memory::static_::{SAFE_STATIC_GUARD_SIZE, SAFE_STATIC_HEAP_SIZE},
    types::{MemoryDescriptor, ValueType},
    units::Pages,
    vm,
};
use std::{
    cell::{Cell, Ref, RefCell, RefMut},
    fmt,
    marker::PhantomData,
    mem,
    ops::{Bound, Deref, DerefMut, Index, RangeBounds},
    ptr,
    rc::Rc,
    slice,
};

pub use self::atomic::Atomic;
pub use self::dynamic::DynamicMemory;
pub use self::static_::{SharedStaticMemory, StaticMemory};
pub use self::view::{Atomically, MemoryView};

mod atomic;
mod dynamic;
mod static_;
mod view;

#[derive(Clone)]
enum MemoryVariant {
    Unshared(UnsharedMemory),
    Shared(SharedMemory),
}

#[derive(Clone)]
pub struct Memory {
    desc: MemoryDescriptor,
    variant: MemoryVariant,
}

impl Memory {
    /// Create a new `Memory` from a [`MemoryDescriptor`]
    ///
    /// [`MemoryDescriptor`]: struct.MemoryDescriptor.html
    ///
    /// Usage:
    ///
    /// ```
    /// # use wasmer_runtime_core::types::MemoryDescriptor;
    /// # use wasmer_runtime_core::memory::Memory;
    /// # use wasmer_runtime_core::error::Result;
    /// # use wasmer_runtime_core::units::Pages;
    /// # fn create_memory() -> Result<()> {
    /// let descriptor = MemoryDescriptor {
    ///     minimum: Pages(10),
    ///     maximum: None,
    ///     shared: false,
    /// };
    ///
    /// let memory = Memory::new(descriptor)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(desc: MemoryDescriptor) -> Result<Self, CreationError> {
        let variant = if !desc.shared {
            MemoryVariant::Unshared(UnsharedMemory::new(desc)?)
        } else {
            MemoryVariant::Shared(SharedMemory::new(desc)?)
        };

        Ok(Memory { desc, variant })
    }

    /// Return the [`MemoryDescriptor`] that this memory
    /// was created with.
    ///
    /// [`MemoryDescriptor`]: struct.MemoryDescriptor.html
    pub fn descriptor(&self) -> MemoryDescriptor {
        self.desc
    }

    /// Grow this memory by the specfied number of pages.
    pub fn grow(&self, delta: Pages) -> Option<Pages> {
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

    pub fn view<T: ValueType, R: RangeBounds<usize>>(&self, range: R) -> Option<MemoryView<T>> {
        let vm::LocalMemory {
            base,
            bound,
            memory: _,
        } = unsafe { *self.vm_local_memory() };

        let range_start = match range.start_bound() {
            Bound::Included(start) => *start,
            Bound::Excluded(start) => *start + 1,
            Bound::Unbounded => 0,
        };

        let range_end = match range.end_bound() {
            Bound::Included(end) => *end + 1,
            Bound::Excluded(end) => *end,
            Bound::Unbounded => bound as usize,
        };

        let length = range_end - range_start;

        let size_in_bytes = mem::size_of::<T>() * length;

        if range_end < range_start || range_start + size_in_bytes >= bound {
            return None;
        }

        Some(unsafe { MemoryView::new(base as _, length as u32) })
    }

    pub fn shared(self) -> Option<SharedMemory> {
        if self.desc.shared {
            Some(SharedMemory { desc: self.desc })
        } else {
            None
        }
    }

    pub(crate) fn vm_local_memory(&self) -> *mut vm::LocalMemory {
        match &self.variant {
            MemoryVariant::Unshared(unshared_mem) => unshared_mem.vm_local_memory(),
            MemoryVariant::Shared(shared_mem) => unimplemented!(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryType {
    Dynamic,
    Static,
    SharedStatic,
}

impl MemoryType {
    #[doc(hidden)]
    pub fn guard_size(self) -> u64 {
        match self {
            MemoryType::Dynamic => DYNAMIC_GUARD_SIZE as u64,
            MemoryType::Static | MemoryType::SharedStatic => SAFE_STATIC_GUARD_SIZE as u64,
        }
    }

    #[doc(hidden)]
    pub fn bounds(self) -> Option<u64> {
        match self {
            MemoryType::Dynamic => None,
            MemoryType::Static | MemoryType::SharedStatic => Some(SAFE_STATIC_HEAP_SIZE as u64),
        }
    }
}

enum UnsharedMemoryStorage {
    Dynamic(Box<DynamicMemory>),
    Static(Box<StaticMemory>),
}

pub struct UnsharedMemory {
    internal: Rc<UnsharedMemoryInternal>,
}

struct UnsharedMemoryInternal {
    storage: RefCell<UnsharedMemoryStorage>,
    local: Cell<vm::LocalMemory>,
}

impl UnsharedMemory {
    pub fn new(desc: MemoryDescriptor) -> Result<Self, CreationError> {
        let mut local = vm::LocalMemory {
            base: ptr::null_mut(),
            bound: 0,
            memory: ptr::null_mut(),
        };

        let storage = match desc.memory_type() {
            MemoryType::Dynamic => {
                UnsharedMemoryStorage::Dynamic(DynamicMemory::new(desc, &mut local)?)
            }
            MemoryType::Static => {
                UnsharedMemoryStorage::Static(StaticMemory::new(desc, &mut local)?)
            }
            MemoryType::SharedStatic => panic!("attempting to create shared unshared memory"),
        };

        Ok(UnsharedMemory {
            internal: Rc::new(UnsharedMemoryInternal {
                storage: RefCell::new(storage),
                local: Cell::new(local),
            }),
        })
    }

    pub fn grow(&self, delta: Pages) -> Option<Pages> {
        let mut storage = self.internal.storage.borrow_mut();

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

    pub fn size(&self) -> Pages {
        let storage = self.internal.storage.borrow();

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
            internal: Rc::clone(&self.internal),
        }
    }
}

pub struct SharedMemory {
    desc: MemoryDescriptor,
}

impl SharedMemory {
    fn new(desc: MemoryDescriptor) -> Result<Self, CreationError> {
        Ok(Self { desc })
    }

    pub fn grow(&self, _delta: Pages) -> Option<Pages> {
        unimplemented!()
    }

    pub fn size(&self) -> Pages {
        unimplemented!()
    }

    pub unsafe fn as_slice(&self) -> &[u8] {
        unimplemented!()
    }

    pub unsafe fn as_slice_mut(&self) -> &mut [u8] {
        unimplemented!()
    }
}

impl Clone for SharedMemory {
    fn clone(&self) -> Self {
        unimplemented!()
    }
}
