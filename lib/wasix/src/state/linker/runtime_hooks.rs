use tracing::warn;
use wasmer::{AsStoreMut, FunctionEnv, Imports, Instance, Memory, Module};

use crate::WasiEnv;

use super::LinkError;

pub(super) fn instantiate_with_runtime_hooks(
    env: &FunctionEnv<WasiEnv>,
    store: &mut impl AsStoreMut,
    module: &Module,
    imports: &mut Imports,
    imported_memory: &Memory,
) -> Result<Instance, LinkError> {
    let runtime = env.as_ref(store).runtime.clone();

    {
        let mut store_mut = store.as_store_mut();
        let additional_imports = runtime
            .additional_imports(module, &mut store_mut)
            .map_err(LinkError::RuntimeHookError)?;
        merge_missing_imports(imports, &additional_imports);
    }

    let instance = Instance::new(store, module, imports)?;

    {
        let mut store_mut = store.as_store_mut();
        runtime
            .configure_new_instance(module, &mut store_mut, &instance, Some(imported_memory))
            .map_err(LinkError::RuntimeHookError)?;
    }

    Ok(instance)
}

fn merge_missing_imports(imports: &mut Imports, additional_imports: &Imports) {
    for ((namespace, name), value) in additional_imports {
        if imports.exists(&namespace, &name) {
            warn!(
                "Skipping duplicate additional import {}.{}",
                namespace, name
            );
        } else {
            imports.define(&namespace, &name, value);
        }
    }
}
