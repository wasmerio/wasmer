use wasmer_runtime_core::module::Module;

#[allow(dead_code)]
/// Check if a provided module is compiled for some version of WASI.
/// Use [`get_wasi_version`] to find out which version of WASI the module is.
pub fn is_wasi_module(module: &Module) -> bool {
    get_wasi_version(module).is_some()
}

/// The version of WASI. This is determined by the imports namespace
/// string.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WasiVersion {
    /// `wasi_unstable`.
    Snapshot0,
    /// `wasi_snapshot_preview1`.
    Snapshot1,

    /// Latest version.
    ///
    /// It's a “floating” version, i.e. it's an alias to the latest
    /// version (for the moment, `Snapshot1`). Using this version is a
    /// way to ensure that modules will run only if they come with the
    /// latest WASI version (in case of security issues for instance),
    /// by just updating the runtime.
    ///
    /// Note that this version is never returned by an API. It is
    /// provided only by the user.
    Latest,
}

/// Detect the version of WASI being used from the namespace
pub fn get_wasi_version(module: &Module) -> Option<WasiVersion> {
    let mut import_iter = module
        .info()
        .imported_functions
        .iter()
        .map(|(_, import_name)| import_name.namespace_index);

    // returns None if empty
    let first = import_iter.next()?;
    if import_iter.all(|idx| idx == first) {
        // once we know that all the namespaces are the same, we can use it to
        // detect which version of WASI this is
        match module.info().namespace_table.get(first) {
            "wasi_unstable" => Some(WasiVersion::Snapshot0),
            "wasi_snapshot_preview1" => Some(WasiVersion::Snapshot1),
            _ => None,
        }
    } else {
        // not all funcs have the same namespace, therefore it's not WASI
        None
    }
}
