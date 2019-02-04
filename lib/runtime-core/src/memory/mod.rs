use crate::{
    error::CreationError,
    export::Export,
    import::IsExport,
    memory::dynamic::DYNAMIC_GUARD_SIZE,
    memory::static_::{SAFE_STATIC_GUARD_SIZE, SAFE_STATIC_HEAP_SIZE},
    types::MemoryDescriptor,
    units::Pages,
    vm,
};
use std::{
    cell::{Cell, Ref, RefCell, RefMut},
    fmt,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    ptr,
    rc::Rc,
};

pub use self::dynamic::DynamicMemory;
pub use self::static_::{SharedStaticMemory, StaticMemory};

mod dynamic;
mod static_;

pub trait MemoryImpl<'a>: Clone {
    type Access: Deref<Target = [u8]>;
    type AccessMut: DerefMut<Target = [u8]>;

    fn new(desc: MemoryDescriptor) -> Result<Self, CreationError>;
    fn grow(&'a self, delta: Pages) -> Option<Pages>;
    fn size(&'a self) -> Pages;
    fn vm_local_memory(&'a self) -> *mut vm::LocalMemory;
    fn access(&'a self) -> Self::Access;
    fn access_mut(&'a self) -> Self::AccessMut;
}

pub trait SharedPolicy
where
    Self: Sized,
    for<'a> Self::Memory: MemoryImpl<'a>,
{
    const SHARED: bool;
    type Memory;
    fn transform_variant(variants: &MemoryVariant) -> &Memory<Self>;
}
pub struct Shared;
impl SharedPolicy for Shared {
    const SHARED: bool = true;
    type Memory = SharedMemory;
    fn transform_variant(variants: &MemoryVariant) -> &Memory<Self> {
        match variants {
            MemoryVariant::Shared(shared_mem) => shared_mem,
            MemoryVariant::Unshared(_) => {
                panic!("cannot transform unshared memory to shared memory")
            }
        }
    }
}
pub struct Unshared;
impl SharedPolicy for Unshared {
    const SHARED: bool = false;
    type Memory = UnsharedMemory;
    fn transform_variant(variants: &MemoryVariant) -> &Memory<Self> {
        match variants {
            MemoryVariant::Unshared(unshared_mem) => unshared_mem,
            MemoryVariant::Shared(_) => panic!("cannot transform shared memory to unshared memory"),
        }
    }
}

unsafe impl Send for Memory<Shared> {}
unsafe impl Sync for Memory<Shared> {}

pub struct Memory<S = Unshared>
where
    S: SharedPolicy,
{
    desc: MemoryDescriptor,
    memory: S::Memory,
    _phantom: PhantomData<S>,
}

impl<S> Memory<S>
where
    S: SharedPolicy,
{
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
    /// let memory: Memory = Memory::new(descriptor)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(desc: MemoryDescriptor) -> Result<Memory<S>, CreationError> {
        assert_eq!(
            desc.shared,
            S::SHARED,
            "type parameter must match description"
        );

        Ok(Memory {
            desc,
            memory: S::Memory::new(desc)?,
            _phantom: PhantomData,
        })
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
        self.memory.grow(delta)
    }

    /// The size, in wasm pages, of this memory.
    pub fn size(&self) -> Pages {
        self.memory.size()
    }

    pub fn access(&self) -> <S::Memory as MemoryImpl>::Access {
        self.memory.access()
    }

    pub fn access_mut(&self) -> <S::Memory as MemoryImpl>::AccessMut {
        self.memory.access_mut()
    }

    pub(crate) fn vm_local_memory(&self) -> *mut vm::LocalMemory {
        self.memory.vm_local_memory()
    }
}

impl IsExport for Memory<Unshared> {
    fn to_export(&self) -> Export {
        Export::Memory(MemoryVariant::Unshared(self.clone()))
    }
}
impl IsExport for Memory<Shared> {
    fn to_export(&self) -> Export {
        Export::Memory(MemoryVariant::Shared(self.clone()))
    }
}

impl<S> Clone for Memory<S>
where
    S: SharedPolicy,
{
    fn clone(&self) -> Self {
        Self {
            desc: self.desc,
            memory: self.memory.clone(),
            _phantom: PhantomData,
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

impl<S> fmt::Debug for Memory<S>
where
    S: SharedPolicy,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Memory")
            .field("desc", &self.desc)
            .field("size", &self.size())
            .finish()
    }
}

#[derive(Debug, Clone)]
pub enum MemoryVariant {
    Unshared(Memory<Unshared>),
    Shared(Memory<Shared>),
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

impl<'a> MemoryImpl<'a> for UnsharedMemory {
    type Access = Ref<'a, [u8]>;
    type AccessMut = RefMut<'a, [u8]>;

    fn new(desc: MemoryDescriptor) -> Result<Self, CreationError> {
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

    fn grow(&self, delta: Pages) -> Option<Pages> {
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

    fn size(&self) -> Pages {
        let storage = self.internal.storage.borrow();

        match &*storage {
            UnsharedMemoryStorage::Dynamic(ref dynamic_memory) => dynamic_memory.size(),
            UnsharedMemoryStorage::Static(ref static_memory) => static_memory.size(),
        }
    }

    fn vm_local_memory(&self) -> *mut vm::LocalMemory {
        self.internal.local.as_ptr()
    }

    fn access(&'a self) -> Ref<'a, [u8]> {
        Ref::map(
            self.internal.storage.borrow(),
            |memory_storage| match memory_storage {
                UnsharedMemoryStorage::Dynamic(dynamic_memory) => dynamic_memory.as_slice(),
                UnsharedMemoryStorage::Static(static_memory) => static_memory.as_slice(),
            },
        )
    }

    fn access_mut(&'a self) -> RefMut<'a, [u8]> {
        RefMut::map(
            self.internal.storage.borrow_mut(),
            |memory_storage| match memory_storage {
                UnsharedMemoryStorage::Dynamic(dynamic_memory) => dynamic_memory.as_slice_mut(),
                UnsharedMemoryStorage::Static(static_memory) => static_memory.as_slice_mut(),
            },
        )
    }
}

impl Clone for UnsharedMemory {
    fn clone(&self) -> Self {
        UnsharedMemory {
            internal: Rc::clone(&self.internal),
        }
    }
}

pub struct SharedMemory {}

impl<'a> MemoryImpl<'a> for SharedMemory {
    type Access = Vec<u8>;
    type AccessMut = Vec<u8>;

    fn new(_desc: MemoryDescriptor) -> Result<Self, CreationError> {
        unimplemented!()
    }

    fn grow(&self, _delta: Pages) -> Option<Pages> {
        unimplemented!()
    }

    fn size(&self) -> Pages {
        unimplemented!()
    }

    fn vm_local_memory(&self) -> *mut vm::LocalMemory {
        unimplemented!()
    }

    fn access(&self) -> Vec<u8> {
        unimplemented!()
    }

    fn access_mut(&self) -> Vec<u8> {
        unimplemented!()
    }
}

impl Clone for SharedMemory {
    fn clone(&self) -> Self {
        unimplemented!()
    }
}
