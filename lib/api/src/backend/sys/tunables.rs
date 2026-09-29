pub use wasmer_compiler::BaseTunables;

// All BaseTunable definition now is in wasmer_compile crate
// Tests are still here

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TableType;
    #[allow(unused)]
    use crate::sys::NativeEngineExt;
    use std::ptr::NonNull;
    use wasmer_compiler::Tunables;
    use wasmer_types::{MemoryType, Pages};
    use wasmer_vm::{
        MemoryError, MemoryStyle, TableStyle, VMConfig, VMMemory, VMMemoryDefinition, VMTable,
        VMTableDefinition,
    };

    #[test]
    fn memory_style() {
        let tunables = BaseTunables::new();

        // No maximum: treat as wasm32 maximum, so static.
        let requested = MemoryType::new(3, None, true);
        let style = tunables.memory_style(&requested);
        match style {
            MemoryStyle::Static => {}
            s => panic!("Unexpected memory style: {s:?}"),
        }

        // Large maximum
        let requested = MemoryType::new(3, Some(5_000_000), true);
        let style = tunables.memory_style(&requested);
        match style {
            MemoryStyle::Static => {}
            s => panic!("Unexpected memory style: {s:?}"),
        }

        // Small maximum
        let requested = MemoryType::new(3, Some(16), true);
        let style = tunables.memory_style(&requested);
        match style {
            MemoryStyle::Static => {}
            s => panic!("Unexpected memory style: {s:?}"),
        }
    }

    struct TinyTunables;
    impl Tunables for TinyTunables {
        fn memory_style(&self, _memory: &MemoryType) -> MemoryStyle {
            MemoryStyle::Static
        }

        /// Construct a `TableStyle` for the provided `TableType`
        fn table_style(&self, _table: &TableType) -> TableStyle {
            TableStyle::CallerChecksSignature
        }
        fn create_host_memory(
            &self,
            ty: &MemoryType,
            style: &MemoryStyle,
        ) -> Result<VMMemory, MemoryError> {
            VMMemory::new(ty, style)
        }
        unsafe fn create_vm_memory(
            &self,
            ty: &MemoryType,
            style: &MemoryStyle,
            vm_definition_location: NonNull<VMMemoryDefinition>,
        ) -> Result<VMMemory, MemoryError> {
            unsafe { VMMemory::from_definition(ty, style, vm_definition_location) }
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
            unsafe { VMTable::from_definition(ty, style, vm_definition_location) }
        }

        // Will use a minimum stack size of 8kb, not the 1Mb default
        fn vmconfig(&self) -> &wasmer_vm::VMConfig {
            &VMConfig {
                wasm_stack_size: Some(8 * 1024),
            }
        }
    }

    #[test]
    #[cfg(any(feature = "cranelift", feature = "llvm", feature = "singlepass"))]
    fn check_custom_tunables() -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(feature = "wat")]
        use crate::wat2wasm;
        use crate::{Engine, Instance, Memory, Module, Store, imports};

        let wasm_bytes = wat2wasm(
            br#"(module
            (memory (;0;) 18)
            (global (;0;) (mut i32) i32.const 1048576)
            (export "memory" (memory 0))
            (data (;0;) (i32.const 1048576) "*\00\00\00")
          )"#,
        )?;

        cfg_select! {
            feature = "singlepass" => {
                let compiler =  wasmer_compiler_singlepass::Singlepass::default();
            }
            feature = "llvm" => {
                let compiler =  wasmer_compiler_llvm::LLVM::default();
            }
            _ => {
                let compiler =  wasmer_compiler_cranelift::Cranelift::default();
            }
        }

        let tunables = TinyTunables {};
        #[allow(deprecated)]
        let mut engine = Engine::new(compiler.into(), Default::default(), Default::default());
        engine.set_tunables(tunables);
        let mut store = Store::new(engine);
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
        assert_eq!(first_memory.ty(&store).maximum, None);
        let view = first_memory.view(&store);
        let x = unsafe { view.data_unchecked_mut() }[0];
        assert_eq!(x, 0);

        Ok(())
    }

    #[test]
    #[cfg(all(
        feature = "singlepass",
        not(any(
            target_os = "windows",
            all(target_os = "macos", target_arch = "aarch64")
        ))
    ))]
    #[allow(clippy::print_stdout)]
    fn check_small_stack() -> Result<(), Box<dyn std::error::Error>> {
        use crate::{Engine, Instance, Module, Store, imports, wat2wasm};
        use wasmer_compiler_singlepass::Singlepass;
        // This test needs Singlepass compiler
        // because Cranelift will optimize the webassembly file
        // and remove all the unused local, even at optimization level "None"
        // But this test needs the huge amount of locals (1024 + a few)
        // so that the small stack is overflown (stack is only 8K, 1024 i64 local = 8K)
        // tWindows is disable as it seems Stack frame protection is not 100% efficient
        let wasm_bytes = wat2wasm(
            br#"(module
                (func (;0;) (result i64)
                  (local i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64)
                  i64.const 0
                  i64.const 5555
                  i64.add
                  local.set 8
                  i64.const 0
                  i64.const 5555
                  i64.add
                  local.set 9
                  i64.const 0
                  i64.const 5555
                  i64.add
                  local.set 10
                  local.get 10
                )
                (func $large_local (export "large_local") (result i64)
                  (local
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64

                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
                   i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64

                   i64
                  )
                  (local.set 0 (i64.const 1))
                  (local.set 1 (i64.const 1))
                  (local.set 2 (i64.const 1))
                  (local.set 3 (i64.const 1))
                  (local.set 1024 (i64.const 2))
                  (call 0)
                  local.set 1024
                  local.get 6
                  local.get 7
                  i64.add
                  local.get 8
                  i64.add
                  (call 0)
                  local.set 10
                  local.get 9
                  i64.add
                  local.get 10
                  i64.add
                  local.get 11
                  i64.add
                  local.get 12
                  i64.add
                  (call 0)
                  local.set 512
                  local.get 13
                  i64.add
                  local.get 14
                  i64.add
                  local.get 15
                  i64.add
                  local.get 1024
                  i64.add
                  local.get 0
                  i64.add
                )
              )
            "#,
        )?;
        let compiler = Singlepass::default();

        let tunables = TinyTunables {};
        #[allow(deprecated)]
        let mut engine = Engine::new(compiler.into(), Default::default(), Default::default());
        engine.set_tunables(tunables);
        let mut store = Store::new(engine);
        let module = Module::new(&store, wasm_bytes)?;
        let import_object = imports! {};
        let instance = Instance::new(&mut store, &module, &import_object)?;

        let result = instance
            .exports
            .get_function("large_local")?
            .call(&mut store, &[]);

        println!("result = {result:?}");
        assert!(result.is_err());

        Ok(())
    }
}
