use wasmer_runtime_core::module::Module;

#[allow(dead_code)]
/// Check if a provided module is compiled for some version of WASI.
/// Use [`get_wasi_version`] to find out which version of WASI the module is.
pub fn is_wasi_module(module: &Module) -> bool {
    get_wasi_version(module).is_some()
}

/// The version of WASI.  This is determined by the namespace string
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WasiVersion {
    /// "wasi_unstable"
    Snapshot0,
    /// "wasi_snapshot_preview1"
    Snapshot1,
}

/// Detect the version of WASI being used from the namespace
pub fn get_wasi_version(module: &Module) -> Option<WasiVersion> {
    let namespace_table = &module.info().namespace_table;

    module
        .info()
        .imported_functions
        .iter()
        .find_map(|(_, import_name)| {
            let namespace_index = import_name.namespace_index;

            match namespace_table.get(namespace_index) {
                "wasi_unstable" => Some(WasiVersion::Snapshot0),
                "wasi_snapshot_preview1" => Some(WasiVersion::Snapshot1),
                _ => None,
            }
        })
}
