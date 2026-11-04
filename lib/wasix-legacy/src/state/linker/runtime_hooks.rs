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

    let instantiation_state = {
        let mut store_mut = store.as_store_mut();
        runtime
            .prepare_imports(module, &mut store_mut, imports)
            .map_err(LinkError::RuntimeHookError)?
    };

    let instance = Instance::new(store, module, imports)?;

    {
        let mut store_mut = store.as_store_mut();
        runtime
            .configure_new_instance(
                module,
                &mut store_mut,
                &instance,
                Some(imported_memory),
                instantiation_state,
            )
            .map_err(LinkError::RuntimeHookError)?;
    }

    Ok(instance)
}
