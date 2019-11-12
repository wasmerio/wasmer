use wasmer_runtime_core::module::Module;

#[allow(dead_code)]
/// Check if a provided module is compiled with WASI support
pub fn is_wasi_module(module: &Module) -> bool {
    get_wasi_version(module) == Some(WasiVersion::Snapshot1)
}

/// The version of WASI.  This is determined by the namespace string
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WasiVersion {
    /// "wasi_unstable"
    Snapshot0,
    /// "wasi_snapshot_preview1"
    Snapshot1,
}

pub fn get_wasi_version(module: &Module) -> Option<WasiVersion> {
    let mut import_iter = module
        .info()
        .imported_functions
        .iter()
        .map(|(_, import_name)| import_name.namespace_index);

    // returns None if empty
    let first = import_iter.next()?;
    if import_iter.all(|idx| idx == first) {
        match module.info().namespace_table.get(first) {
            "wasi_unstable" => Some(WasiVersion::Snapshot0),
            "wasi_snapshot_preview1" => Some(WasiVersion::Snapshot1),
            _ => None,
        }
    } else {
        // not all funcs have the same namespace: therefore it's not WASI
        None
    }
}
