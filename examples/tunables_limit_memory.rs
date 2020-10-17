use std::sync::Arc;
use wasmer::{
    imports, wat2wasm, Instance, Memory, MemoryError, MemoryType, Module, Pages, Store, TableType,
    Target, Tunables as BaseTunables,
};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_engine::Tunables;
use wasmer_engine_jit::JIT;
use wasmer_vm::{Memory as MemoryTrait, MemoryStyle, Table, TableStyle};

/// A custom tunables that allows you to set a memory limit
struct LimitingTunables {
    /// The maxium a linear memory is allowed to be (in Wasm pages, 65 KiB each).
    /// Since Wasmer ensures there is only none or one memory, this is practically
    /// an upper limit for the guest memory.
    max_memory: Pages,
    /// The base implementation we delegate all the logic to
    base: BaseTunables,
}

impl LimitingTunables {
    pub fn for_target(target: &Target, limit: Pages) -> Self {
        Self {
            max_memory: limit,
            base: BaseTunables::for_target(target),
        }
    }
}

impl Tunables for LimitingTunables {
    /// Construct a `MemoryStyle` for the provided `MemoryType`
    ///
    /// Delegated to base.
    fn memory_style(&self, memory: &MemoryType) -> MemoryStyle {
        self.base.memory_style(memory)
    }

    /// Construct a `TableStyle` for the provided `TableType`
    ///
    /// Delegated to base.
    fn table_style(&self, table: &TableType) -> TableStyle {
        self.base.table_style(table)
    }

    /// Create a memory given a memory type
    ///
    /// The requested memory type is validated, adjusted to the limited and then passed to base.
    fn create_memory(
        &self,
        requested: &MemoryType,
        style: &MemoryStyle,
    ) -> Result<Arc<dyn MemoryTrait>, MemoryError> {
        if requested.minimum > self.max_memory {
            return Err(MemoryError::Generic(
                "Minimum of requested memory exceeds the allowed memory limit".to_string(),
            ));
        }

        if let Some(max) = requested.maximum {
            if max > self.max_memory {
                return Err(MemoryError::Generic(
                    "Maximum of requested memory exceeds the allowed memory limit".to_string(),
                ));
            }
        }

        let mut adjusted = requested.clone();
        adjusted.maximum = Some(self.max_memory);

        self.base.create_memory(&adjusted, style)
    }

    /// Create a memory given a memory type
    ///
    /// Delegated to base.
    fn create_table(&self, ty: &TableType, style: &TableStyle) -> Result<Arc<dyn Table>, String> {
        self.base.create_table(ty, style)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A Wasm module with one exported memory (min: 7 pages, max: unset)
    let wat = br#"(module (memory 7) (export "memory" (memory 0)))"#;

    // Alternatively: A Wasm module with one exported memory (min: 7 pages, max: 80 pages)
    // let wat = br#"(module (memory 7 80) (export "memory" (memory 0)))"#;

    let wasm_bytes = wat2wasm(wat)?;

    // Any compiler and any engine do the job here
    let compiler = Cranelift::default();
    let engine = JIT::new(&compiler).engine();

    // Here is where the fun begins

    let target = Target::default(); // TODO: should this use engine.target(), which is private?
    let tunables = LimitingTunables::for_target(&target, Pages(24));

    // Create a store, that holds the engine and our custom tunables
    let store = Store::new_with_tunables(&engine, tunables);

    println!("Compiling module...");
    let module = Module::new(&store, wasm_bytes)?;

    println!("Instantiating module...");
    let import_object = imports! {};

    // Now at this point, our custom tunables are used
    let instance = Instance::new(&module, &import_object)?;

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
    assert_eq!(first_memory.ty().maximum.unwrap(), Pages(24));

    Ok(())
}

#[test]
fn test_tunables_limit_memory() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
