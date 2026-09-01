//! Integration with the WASIX runtime's instantiation hooks.

use anyhow::{Context, Result};
use wasmer_api::{Imports, Instance, Memory, Module, StoreMut};
use wasmer_wasix::runtime::{InstantiationHook, InstantiationState};

use crate::{WasmCapiInstantiationState, WasmCapiRuntimeHooks};

/// Lets WASIX embedders register the hooks directly, e.g. through
/// `PluggableRuntime::with_instantiation_hook`.
impl InstantiationHook for WasmCapiRuntimeHooks {
    fn additional_imports(
        &self,
        module: &Module,
        store: &mut StoreMut,
    ) -> Result<(Imports, InstantiationState)> {
        let (imports, state) = WasmCapiRuntimeHooks::additional_imports(self, module, store)?;
        Ok((imports, InstantiationState::new(state)))
    }

    fn prepare_imports(
        &self,
        module: &Module,
        store: &mut StoreMut,
        imports: &mut Imports,
    ) -> Result<InstantiationState> {
        let state = WasmCapiRuntimeHooks::add_imports(self, module, store, imports)?;
        Ok(InstantiationState::new(state))
    }

    fn configure_new_instance(
        &self,
        module: &Module,
        store: &mut StoreMut,
        instance: &Instance,
        imported_memory: Option<&Memory>,
        state: InstantiationState,
    ) -> Result<()> {
        let state = state
            .take::<WasmCapiInstantiationState>()
            .context("invalid Wasm C API instance setup state")?;
        WasmCapiRuntimeHooks::configure_instance(
            self,
            module,
            store,
            instance,
            imported_memory,
            state,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmer_api::{AsStoreMut, MemoryType, Pages, Store};

    #[test]
    fn prepare_imports_reuses_wasix_memory_before_start() {
        let mut store = Store::default();
        let module = Module::new(
            &store,
            r#"(module
                (import "env" "memory" (memory 1 1))
                (import "wasm_c_api_v0" "wasm_byte_vec_new"
                    (func $wasm_byte_vec_new (param i32 i32 i32)))
                (data (i32.const 16) "\ff\ff\ff\ff\ff\ff\ff\ff")
                (func $initialize
                    (call $wasm_byte_vec_new
                        (i32.const 16)
                        (i32.const 0)
                        (i32.const 0)))
                (start $initialize)
            )"#,
        )
        .expect("module compiles");
        let memory = Memory::new(&mut store, MemoryType::new(Pages(1), Some(Pages(1)), false))
            .expect("WASIX memory can be created");
        let mut imports = Imports::new();
        imports.define("env", "memory", memory.clone());

        let hooks = WasmCapiRuntimeHooks::new();
        let _state = InstantiationHook::prepare_imports(
            &hooks,
            &module,
            &mut store.as_store_mut(),
            &mut imports,
        )
        .expect("C API imports can be prepared");
        Instance::new(&mut store, &module, &imports).expect("start function succeeds");

        let mut header = [0xff; 8];
        memory
            .view(&store)
            .read(16, &mut header)
            .expect("vector header can be read");
        assert_eq!(header, [0; 8]);
    }
}
