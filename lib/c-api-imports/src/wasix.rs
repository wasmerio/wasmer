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
