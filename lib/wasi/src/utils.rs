use wasmer_runtime_core::module::Module;

#[allow(dead_code)]
/// Check if a provided module is compiled for some version of WASI.
/// Use [`get_wasi_version`] to find out which version of WASI the module is.
pub fn is_wasi_module(module: &Module) -> bool {
    get_wasi_version(module, false).is_some()
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

/// Namespace for the `Snapshot0` version.
const SNAPSHOT0_NAMESPACE: &'static str = "wasi_unstable";

/// Namespace for the `Snapshot1` version.
const SNAPSHOT1_NAMESPACE: &'static str = "wasi_snapshot_preview1";

/// Detect the version of WASI being used based on the import
/// namespaces.
///
/// A strict detection expects that all imports live in a single WASI
/// namespace. A non-strict detection expects that at least one WASI
/// namespace exits to detect the version. Note that the strict
/// detection is faster than the non-strict one.
pub fn get_wasi_version(module: &Module, strict: bool) -> Option<WasiVersion> {
    let module_info = &module.info();
    let mut imports = module_info.imported_functions.iter();

    if strict {
        let mut imports = imports.map(|(_, import_name)| import_name.namespace_index);

        // Returns `None` if empty.
        let first = imports.next()?;

        // If there is only one namespace…
        if imports.all(|index| index == first) {
            // … and that this namespace is a WASI one.
            match module_info.namespace_table.get(first) {
                SNAPSHOT0_NAMESPACE => Some(WasiVersion::Snapshot0),
                SNAPSHOT1_NAMESPACE => Some(WasiVersion::Snapshot1),
                _ => None,
            }
        } else {
            None
        }
    } else {
        let namespace_table = &module_info.namespace_table;

        // Check that at least a WASI namespace exists, and use the
        // first one in the list to detect the WASI version.
        imports.find_map(|(_, import_name)| {
            let namespace_index = import_name.namespace_index;

            match namespace_table.get(namespace_index) {
                SNAPSHOT0_NAMESPACE => Some(WasiVersion::Snapshot0),
                SNAPSHOT1_NAMESPACE => Some(WasiVersion::Snapshot1),
                _ => None,
            }
        })
    }
}
