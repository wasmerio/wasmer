use wasmer_runtime_core::module::Module;

/// Check if a provided module is compiled with WASI support
pub fn is_wasi_module(module: &Module) -> bool {
    for (_, import_name) in &module.info().imported_functions {
        let namespace = module
            .info()
            .namespace_table
            .get(import_name.namespace_index);
        if namespace == "wasi_unstable" {
            return true;
        }
    }
    false
}
