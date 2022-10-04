use crate::sys::{MemoryType, Pages, TableType};
use std::ptr::NonNull;
use wasmer_compiler::Tunables;
use wasmer_types::{PointerWidth, Target};
use wasmer_vm::MemoryError;
use wasmer_vm::{
    MemoryStyle, TableStyle, VMMemory, VMMemoryDefinition, VMTable, VMTableDefinition,
};

/// Tunable parameters for WebAssembly compilation.
/// This is the reference implementation of the `Tunables` trait,
/// used by default.
///
/// You can use this as a template for creating a custom Tunables
/// implementation or use composition to wrap your Tunables around
/// this one. The later approach is demonstrated in the
/// tunables-limit-memory example.
#[derive(Clone)]
pub struct BaseTunables {
    /// For static heaps, the size in wasm pages of the heap protected by bounds checking.
    pub static_memory_bound: Pages,

    /// The size in bytes of the offset guard for static heaps.
    pub static_memory_offset_guard_size: u64,

    /// The size in bytes of the offset guard for dynamic heaps.
    pub dynamic_memory_offset_guard_size: u64,
}

impl BaseTunables {
    /// Get the `BaseTunables` for a specific Target
    pub fn for_target(target: &Target) -> Self {
        let triple = target.triple();
        let pointer_width: PointerWidth = triple.pointer_width().unwrap();
        let (static_memory_bound, static_memory_offset_guard_size): (Pages, u64) =
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
        // The Windows memory manager seems more laxed than the other ones
        // And a guard of just 1 page may not be enough is some borderline cases
        // So using 2 pages for guard on this platform
        #[cfg(target_os = "windows")]
        let dynamic_memory_offset_guard_size: u64 = 0x2_0000;
        #[cfg(not(target_os = "windows"))]
        let dynamic_memory_offset_guard_size: u64 = 0x1_0000;

        Self {
            static_memory_bound,
            static_memory_offset_guard_size,
            dynamic_memory_offset_guard_size,
        }
    }
}

impl Tunables for BaseTunables {
    /// Get a `MemoryStyle` for the provided `MemoryType`
    fn memory_style(&self, memory: &MemoryType) -> MemoryStyle {
        // A heap with a maximum that doesn't exceed the static memory bound specified by the
        // tunables make it static.
        //
        // If the module doesn't declare an explicit maximum treat it as 4GiB.
        let maximum = memory.maximum.unwrap_or_else(Pages::max_value);
        if maximum <= self.static_memory_bound {
            MemoryStyle::Static {
                // Bound can be larger than the maximum for performance reasons
                bound: self.static_memory_bound,
                offset_guard_size: self.static_memory_offset_guard_size,
            }
        } else {
            MemoryStyle::Dynamic {
                offset_guard_size: self.dynamic_memory_offset_guard_size,
            }
        }
    }

    /// Get a [`TableStyle`] for the provided [`TableType`].
    fn table_style(&self, _table: &TableType) -> TableStyle {
        TableStyle::CallerChecksSignature
    }

    /// Create a memory owned by the host given a [`MemoryType`] and a [`MemoryStyle`].
    fn create_host_memory(
        &self,
        ty: &MemoryType,
        style: &MemoryStyle,
    ) -> Result<VMMemory, MemoryError> {
        VMMemory::new(ty, style)
    }

    /// Create a memory owned by the VM given a [`MemoryType`] and a [`MemoryStyle`].
    ///
    /// # Safety
    /// - `vm_definition_location` must point to a valid, owned `VMMemoryDefinition`,
    ///   for example in `VMContext`.
    unsafe fn create_vm_memory(
        &self,
        ty: &MemoryType,
        style: &MemoryStyle,
        vm_definition_location: NonNull<VMMemoryDefinition>,
    ) -> Result<VMMemory, MemoryError> {
        VMMemory::from_definition(ty, style, vm_definition_location)
    }

    /// Create a table owned by the host given a [`TableType`] and a [`TableStyle`].
    fn create_host_table(&self, ty: &TableType, style: &TableStyle) -> Result<VMTable, String> {
        VMTable::new(ty, style)
    }

    /// Create a table owned by the VM given a [`TableType`] and a [`TableStyle`].
    ///
    /// # Safety
    /// - `vm_definition_location` must point to a valid, owned `VMTableDefinition`,
    ///   for example in `VMContext`.
    unsafe fn create_vm_table(
        &self,
        ty: &TableType,
        style: &TableStyle,
        vm_definition_location: NonNull<VMTableDefinition>,
    ) -> Result<VMTable, String> {
        VMTable::from_definition(ty, style, vm_definition_location)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_style() {
        let tunables = BaseTunables {
            static_memory_bound: Pages(2048),
            static_memory_offset_guard_size: 128,
            dynamic_memory_offset_guard_size: 256,
        };

        // No maximum
        let requested = MemoryType::new(3, None, true);
        let style = tunables.memory_style(&requested);
        match style {
            MemoryStyle::Dynamic { offset_guard_size } => assert_eq!(offset_guard_size, 256),
            s => panic!("Unexpected memory style: {:?}", s),
        }

        // Large maximum
        let requested = MemoryType::new(3, Some(5_000_000), true);
        let style = tunables.memory_style(&requested);
        match style {
            MemoryStyle::Dynamic { offset_guard_size } => assert_eq!(offset_guard_size, 256),
            s => panic!("Unexpected memory style: {:?}", s),
        }

        // Small maximum
        let requested = MemoryType::new(3, Some(16), true);
        let style = tunables.memory_style(&requested);
        match style {
            MemoryStyle::Static {
                bound,
                offset_guard_size,
            } => {
                assert_eq!(bound, Pages(2048));
                assert_eq!(offset_guard_size, 128);
            }
            s => panic!("Unexpected memory style: {:?}", s),
        }
    }

    use std::cell::UnsafeCell;
    use std::ptr::NonNull;
    use wasmer_types::{MemoryError, MemoryStyle, MemoryType, Pages, WASM_PAGE_SIZE};
    use wasmer_vm::LinearMemory;

    #[derive(Debug)]
    struct VMTinyMemory {
        mem: Vec<u8>,
        memory_definition: Option<UnsafeCell<VMMemoryDefinition>>,
    }

    unsafe impl Send for VMTinyMemory {}
    unsafe impl Sync for VMTinyMemory {}

    impl VMTinyMemory {
        pub fn new() -> Result<Self, MemoryError> {
            let sz = 18 * WASM_PAGE_SIZE;
            let mut memory = Vec::new();
            memory.resize(sz, 0);
            let mut ret = VMTinyMemory {
                mem: memory,
                memory_definition: None,
            };
            ret.memory_definition = Some(UnsafeCell::new(VMMemoryDefinition {
                base: ret.mem.as_ptr() as _,
                current_length: sz,
            }));
            Ok(ret)
        }
    }

    impl LinearMemory for VMTinyMemory {
        fn ty(&self) -> MemoryType {
            MemoryType {
                minimum: Pages::from(18u32),
                maximum: Some(Pages::from(18u32)),
                shared: false,
            }
        }
        fn size(&self) -> Pages {
            Pages::from(18u32)
        }
        fn style(&self) -> MemoryStyle {
            MemoryStyle::Static {
                bound: Pages::from(18u32),
                offset_guard_size: 0,
            }
        }
        fn grow(&mut self, delta: Pages) -> Result<Pages, MemoryError> {
            Err(MemoryError::CouldNotGrow {
                current: Pages::from(100u32),
                attempted_delta: delta,
            })
        }
        fn vmmemory(&self) -> NonNull<VMMemoryDefinition> {
            unsafe {
                NonNull::new(
                    self.memory_definition
                        .as_ref()
                        .unwrap()
                        .get()
                        .as_mut()
                        .unwrap() as _,
                )
                .unwrap()
            }
        }
        fn try_clone(&self) -> Option<Box<dyn LinearMemory + 'static>> {
            None
        }
        /*
        // this code allow custom memory to be ignoring init_memory
        use wasmer_vm::Trap;
        unsafe fn initialize_with_data(&self, _start: usize, _data: &[u8]) -> Result<(), Trap> {
            Ok(())
        }
        */
    }

    impl From<VMTinyMemory> for wasmer_vm::VMMemory {
        fn from(mem: VMTinyMemory) -> Self {
            Self(Box::new(mem))
        }
    }

    struct TinyTunables;
    impl Tunables for TinyTunables {
        fn memory_style(&self, _memory: &MemoryType) -> MemoryStyle {
            MemoryStyle::Static {
                bound: Pages::from(18u32),
                offset_guard_size: 0,
            }
        }

        /// Construct a `TableStyle` for the provided `TableType`
        fn table_style(&self, _table: &TableType) -> TableStyle {
            TableStyle::CallerChecksSignature
        }
        fn create_host_memory(
            &self,
            _ty: &MemoryType,
            _style: &MemoryStyle,
        ) -> Result<VMMemory, MemoryError> {
            let memory = VMTinyMemory::new().unwrap();
            Ok(VMMemory::from_custom(memory))
        }
        unsafe fn create_vm_memory(
            &self,
            _ty: &MemoryType,
            _style: &MemoryStyle,
            vm_definition_location: NonNull<VMMemoryDefinition>,
        ) -> Result<VMMemory, MemoryError> {
            let memory = VMTinyMemory::new().unwrap();
            // now, it's important to update vm_definition_location with the memory information!
            let mut ptr = vm_definition_location;
            let md = ptr.as_mut();
            let unsafecell = memory.memory_definition.as_ref().unwrap();
            let def = unsafecell.get().as_ref().unwrap();
            md.base = def.base;
            md.current_length = def.current_length;
            Ok(memory.into())
        }

        /// Create a table owned by the host given a [`TableType`] and a [`TableStyle`].
        fn create_host_table(&self, ty: &TableType, style: &TableStyle) -> Result<VMTable, String> {
            VMTable::new(ty, style)
        }

        /// Create a table owned by the VM given a [`TableType`] and a [`TableStyle`].
        ///
        /// # Safety
        /// - `vm_definition_location` must point to a valid location in VM memory.
        unsafe fn create_vm_table(
            &self,
            ty: &TableType,
            style: &TableStyle,
            vm_definition_location: NonNull<VMTableDefinition>,
        ) -> Result<VMTable, String> {
            VMTable::from_definition(ty, style, vm_definition_location)
        }
    }

    #[test]
    fn check_linearmemory() {
        let tunables = TinyTunables {};
        let vmmemory = tunables.create_host_memory(
            &MemoryType::new(1u32, Some(100u32), true),
            &MemoryStyle::Static {
                bound: Pages::from(18u32),
                offset_guard_size: 0u64,
            },
        );
        let mut vmmemory = vmmemory.unwrap();
        assert!(vmmemory.grow(Pages::from(50u32)).is_err());
        assert_eq!(vmmemory.size(), Pages::from(18u32));
        assert_eq!(
            vmmemory.grow(Pages::from(0u32)).err().unwrap(),
            MemoryError::CouldNotGrow {
                current: Pages::from(100u32),
                attempted_delta: Pages::from(0u32)
            }
        );
    }

    #[test]
    fn check_customtunables() -> Result<(), Box<dyn std::error::Error>> {
        use crate::{imports, wat2wasm, Instance, Memory, Module, Store};
        use wasmer_compiler_cranelift::Cranelift;

        let wasm_bytes = wat2wasm(
            br#"(module
            (memory (;0;) 18)
            (global (;0;) (mut i32) i32.const 1048576)
            (export "memory" (memory 0))
            (data (;0;) (i32.const 1048576) "*\00\00\00")
          )"#,
        )?;
        let compiler = Cranelift::default();

        let tunables = TinyTunables {};
        let mut store = Store::new_with_tunables(compiler, tunables);
        //let mut store = Store::new(compiler);
        let module = Module::new(&store, wasm_bytes)?;
        let import_object = imports! {};
        let instance = Instance::new(&mut store, &module, &import_object)?;

        let mut memories: Vec<Memory> = instance
            .exports
            .iter()
            .memories()
            .map(|pair| pair.1.clone())
            .collect();
        assert_eq!(memories.len(), 1);
        let first_memory = memories.pop().unwrap();
        assert_eq!(first_memory.ty(&store).maximum.unwrap(), Pages(18));
        let view = first_memory.view(&store);
        let x = unsafe { view.data_unchecked_mut() }[0];
        assert_eq!(x, 0);

        Ok(())
    }
}
