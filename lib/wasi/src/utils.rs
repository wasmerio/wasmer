use wasmer::{ExternType, Module};

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
const SNAPSHOT0_NAMESPACE: &str = "wasi_unstable";

/// Namespace for the `Snapshot1` version.
const SNAPSHOT1_NAMESPACE: &str = "wasi_snapshot_preview1";

/// Detect the version of WASI being used based on the import
/// namespaces.
///
/// A strict detection expects that all imports live in a single WASI
/// namespace. A non-strict detection expects that at least one WASI
/// namespace exits to detect the version. Note that the strict
/// detection is faster than the non-strict one.
pub fn get_wasi_version(module: &Module, strict: bool) -> Option<WasiVersion> {
    let mut imports = module.imports().filter_map(|extern_| match extern_.ty() {
        ExternType::Function(_f) => Some(extern_.module().to_owned()),
        _ => None,
    });

    if strict {
        let first_module = imports.next()?;
        if imports.all(|module| module == first_module) {
            match first_module.as_str() {
                SNAPSHOT0_NAMESPACE => Some(WasiVersion::Snapshot0),
                SNAPSHOT1_NAMESPACE => Some(WasiVersion::Snapshot1),
                _ => None,
            }
        } else {
            None
        }
    } else {
        // Check that at least a WASI namespace exists, and use the
        // first one in the list to detect the WASI version.
        imports.find_map(|module| match module.as_str() {
            SNAPSHOT0_NAMESPACE => Some(WasiVersion::Snapshot0),
            SNAPSHOT1_NAMESPACE => Some(WasiVersion::Snapshot1),
            _ => None,
        })
    }
}
