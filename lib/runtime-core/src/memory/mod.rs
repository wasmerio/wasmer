use crate::{
    export::Export,
    import::IsExport,
    memory::dynamic::DYNAMIC_GUARD_SIZE,
    memory::static_::{SAFE_STATIC_GUARD_SIZE, SAFE_STATIC_HEAP_SIZE},
    types::MemoryDesc,
    vm,
};
use std::{cell::UnsafeCell, fmt, ptr, rc::Rc};

pub use self::dynamic::DynamicMemory;
pub use self::static_::{SharedStaticMemory, StaticMemory};

mod dynamic;
mod static_;

pub const WASM_PAGE_SIZE: usize = 65_536;
pub const WASM_MAX_PAGES: usize = 65_536;

pub struct Memory {
    desc: MemoryDesc,
    storage: Rc<UnsafeCell<(MemoryStorage, Box<vm::LocalMemory>)>>,
}

impl Memory {
    pub fn new(desc: MemoryDesc) -> Option<Self> {
        let mut vm_local_memory = Box::new(vm::LocalMemory {
            base: ptr::null_mut(),
            bound: 0,
            memory: ptr::null_mut(),
        });

        let memory_storage = match desc.memory_type() {
            MemoryType::Dynamic => {
                MemoryStorage::Dynamic(DynamicMemory::new(desc, &mut vm_local_memory)?)
            }
            MemoryType::Static => {
                MemoryStorage::Static(StaticMemory::new(desc, &mut vm_local_memory)?)
            }
            MemoryType::SharedStatic => unimplemented!(),
        };

        Some(Memory {
            desc,
            storage: Rc::new(UnsafeCell::new((memory_storage, vm_local_memory))),
        })
    }

    pub fn description(&self) -> MemoryDesc {
        self.desc
    }

    pub fn grow(&mut self, delta: u32) -> Option<u32> {
        match unsafe { &mut *self.storage.get() } {
            (MemoryStorage::Dynamic(dynamic_memory), local) => dynamic_memory.grow(delta, local),
            (MemoryStorage::Static(static_memory), local) => static_memory.grow(delta, local),
            (MemoryStorage::SharedStatic(_), _) => unimplemented!(),
        }
    }

    /// This returns the number of pages in the memory.
    pub fn current_pages(&self) -> u32 {
        match unsafe { &*self.storage.get() } {
            (MemoryStorage::Dynamic(dynamic_memory), _) => dynamic_memory.current(),
            (MemoryStorage::Static(static_memory), _) => static_memory.current(),
            (MemoryStorage::SharedStatic(_), _) => unimplemented!(),
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        match unsafe { &*self.storage.get() } {
            (MemoryStorage::Dynamic(dynamic_memory), _) => dynamic_memory.as_slice(),
            (MemoryStorage::Static(static_memory), _) => static_memory.as_slice(),
            (MemoryStorage::SharedStatic(_), _) => panic!("cannot slice a shared memory"),
        }
    }

    pub fn as_slice_mut(&mut self) -> &mut [u8] {
        match unsafe { &mut *self.storage.get() } {
            (MemoryStorage::Dynamic(dynamic_memory), _) => dynamic_memory.as_slice_mut(),
            (MemoryStorage::Static(static_memory), _) => static_memory.as_slice_mut(),
            (MemoryStorage::SharedStatic(_), _) => panic!("cannot slice a shared memory"),
        }
    }

    pub(crate) fn vm_local_memory(&mut self) -> *mut vm::LocalMemory {
        &mut *unsafe { &mut *self.storage.get() }.1
    }
}

impl IsExport for Memory {
    fn to_export(&mut self) -> Export {
        Export::Memory(self.clone())
    }
}

impl Clone for Memory {
    fn clone(&self) -> Self {
        Self {
            desc: self.desc,
            storage: Rc::clone(&self.storage),
        }
    }
}

pub enum MemoryStorage {
    Dynamic(Box<DynamicMemory>),
    Static(Box<StaticMemory>),
    SharedStatic(Box<SharedStaticMemory>),
}

impl MemoryStorage {
    pub fn to_type(&self) -> MemoryType {
        match self {
            MemoryStorage::Dynamic(_) => MemoryType::Dynamic,
            MemoryStorage::Static(_) => MemoryType::Static,
            MemoryStorage::SharedStatic(_) => MemoryType::SharedStatic,
        }
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
            MemoryType::Static => SAFE_STATIC_GUARD_SIZE as u64,
            MemoryType::SharedStatic => SAFE_STATIC_GUARD_SIZE as u64,
        }
    }

    #[doc(hidden)]
    pub fn bounds(self) -> Option<u64> {
        match self {
            MemoryType::Dynamic => None,
            MemoryType::Static => Some(SAFE_STATIC_HEAP_SIZE as u64),
            MemoryType::SharedStatic => Some(SAFE_STATIC_HEAP_SIZE as u64),
        }
    }
}

impl fmt::Debug for Memory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Memory")
            .field("desc", &self.desc)
            .field("size", &(self.current_pages() as usize * WASM_PAGE_SIZE))
            .finish()
    }
}
