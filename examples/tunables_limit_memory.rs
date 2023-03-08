use std::ptr::NonNull;

use wasmer::{
    imports,
    vm::{self, MemoryError, MemoryStyle, TableStyle, VMMemoryDefinition, VMTableDefinition},
    wat2wasm, BaseTunables, Engine, Instance, Memory, MemoryType, Module, Pages, Store, TableType,
    Target, Tunables,
};
use wasmer_compiler_cranelift::Cranelift;
// This is to be able to set the tunables
use wasmer::NativeEngineExt;

/// A custom tunables that allows you to set a memory limit.
///
/// After adjusting the memory limits, it delegates all other logic
/// to the base tunables.
pub struct LimitingTunables<T: Tunables> {
    /// The maximum a linear memory is allowed to be (in Wasm pages, 64 KiB each).
    /// Since Wasmer ensures there is only none or one memory, this is practically
    /// an upper limit for the guest memory.
    limit: Pages,
    /// The base implementation we delegate all the logic to
    base: T,
}

impl<T: Tunables> LimitingTunables<T> {
    pub fn new(base: T, limit: Pages) -> Self {
        Self { limit, base }
    }

    /// Takes an input memory type as requested by the guest and sets
    /// a maximum if missing. The resulting memory type is final if
    /// valid. However, this can produce invalid types, such that
    /// validate_memory must be called before creating the memory.
    fn adjust_memory(&self, requested: &MemoryType) -> MemoryType {
        let mut adjusted = requested.clone();
        if requested.maximum.is_none() {
            adjusted.maximum = Some(self.limit);
        }
        adjusted
    }

    /// Ensures the a given memory type does not exceed the memory limit.
    /// Call this after adjusting the memory.
    fn validate_memory(&self, ty: &MemoryType) -> Result<(), MemoryError> {
        if ty.minimum > self.limit {
            return Err(MemoryError::Generic(
                "Minimum exceeds the allowed memory limit".to_string(),
            ));
        }

        if let Some(max) = ty.maximum {
            if max > self.limit {
                return Err(MemoryError::Generic(
                    "Maximum exceeds the allowed memory limit".to_string(),
                ));
            }
        } else {
            return Err(MemoryError::Generic("Maximum unset".to_string()));
        }

        Ok(())
    }
}

impl<T: Tunables> Tunables for LimitingTunables<T> {
    /// Construct a `MemoryStyle` for the provided `MemoryType`
    ///
    /// Delegated to base.
    fn memory_style(&self, memory: &MemoryType) -> MemoryStyle {
        let adjusted = self.adjust_memory(memory);
        self.base.memory_style(&adjusted)
    }

    /// Construct a `TableStyle` for the provided `TableType`
    ///
    /// Delegated to base.
    fn table_style(&self, table: &TableType) -> TableStyle {
        self.base.table_style(table)
    }

    /// Create a memory owned by the host given a [`MemoryType`] and a [`MemoryStyle`].
    ///
    /// The requested memory type is validated, adjusted to the limited and then passed to base.
    fn create_host_memory(
        &self,
        ty: &MemoryType,
        style: &MemoryStyle,
    ) -> Result<vm::VMMemory, MemoryError> {
        let adjusted = self.adjust_memory(ty);
        self.validate_memory(&adjusted)?;
        self.base.create_host_memory(&adjusted, style)
    }

    /// Create a memory owned by the VM given a [`MemoryType`] and a [`MemoryStyle`].
    ///
    /// Delegated to base.
    unsafe fn create_vm_memory(
        &self,
        ty: &MemoryType,
        style: &MemoryStyle,
        vm_definition_location: NonNull<VMMemoryDefinition>,
    ) -> Result<vm::VMMemory, MemoryError> {
        let adjusted = self.adjust_memory(ty);
        self.validate_memory(&adjusted)?;
        self.base
            .create_vm_memory(&adjusted, style, vm_definition_location)
    }

    /// Create a table owned by the host given a [`TableType`] and a [`TableStyle`].
    ///
    /// Delegated to base.
    fn create_host_table(&self, ty: &TableType, style: &TableStyle) -> Result<vm::VMTable, String> {
        self.base.create_host_table(ty, style)
    }

    /// Create a table owned by the VM given a [`TableType`] and a [`TableStyle`].
    ///
    /// Delegated to base.
    unsafe fn create_vm_table(
        &self,
        ty: &TableType,
        style: &TableStyle,
        vm_definition_location: NonNull<VMTableDefinition>,
    ) -> Result<vm::VMTable, String> {
        self.base.create_vm_table(ty, style, vm_definition_location)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A Wasm module with one exported memory (min: 7 pages, max: unset)
    let wat = br#"(module (memory 7) (export "memory" (memory 0)))"#;

    // Alternatively: A Wasm module with one exported memory (min: 7 pages, max: 80 pages)
    // let wat = br#"(module (memory 7 80) (export "memory" (memory 0)))"#;

    let wasm_bytes = wat2wasm(wat)?;

    // Any compiler do the job here
    let compiler = Cranelift::default();

    // Here is where the fun begins
    let base = BaseTunables::for_target(&Target::default());
    let tunables = LimitingTunables::new(base, Pages(24));
    let mut engine: Engine = compiler.into();
    engine.set_tunables(tunables);

    // Create a store, that holds the engine and our custom tunables
    let mut store = Store::new(engine);

    println!("Compiling module...");
    let module = Module::new(&store, wasm_bytes)?;

    println!("Instantiating module...");
    let import_object = imports! {};

    // Now at this point, our custom tunables are used
    let instance = Instance::new(&mut store, &module, &import_object)?;

    // Check what happened
    let mut memories: Vec<Memory> = instance
        .exports
        .iter()
        .memories()
        .map(|pair| pair.1.clone())
        .collect();
    assert_eq!(memories.len(), 1);

    let first_memory = memories.pop().unwrap();
    println!("Memory of this instance: {:?}", first_memory);
    assert_eq!(first_memory.ty(&store).maximum.unwrap(), Pages(24));

    Ok(())
}

#[test]
fn test_tunables_limit_memory() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
