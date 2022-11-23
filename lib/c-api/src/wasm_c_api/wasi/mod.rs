//! Unofficial API for WASI integrating with the standard Wasm C API.
//!
//! This API will be superseded by a standard WASI API when/if such a standard is created.

pub use super::unstable::wasi::wasi_get_unordered_imports;
use super::{
    externals::{wasm_extern_t, wasm_extern_vec_t, wasm_func_t, wasm_memory_t},
    instance::wasm_instance_t,
    module::wasm_module_t,
    store::{wasm_store_t, StoreRef},
    types::wasm_byte_vec_t,
};
use crate::error::update_last_error;
use std::convert::TryFrom;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::slice;
#[cfg(feature = "webc_runner")]
use wasmer_api::{AsStoreMut, Imports, Module};
use wasmer_wasi::{
    get_wasi_version, Pipe, WasiFile, WasiFunctionEnv, WasiState, WasiStateBuilder, WasiVersion,
};

#[derive(Debug)]
#[allow(non_camel_case_types)]
pub struct wasi_config_t {
    inherit_stdout: bool,
    inherit_stderr: bool,
    inherit_stdin: bool,
    state_builder: WasiStateBuilder,
}

#[no_mangle]
pub unsafe extern "C" fn wasi_config_new(
    program_name: *const c_char,
) -> Option<Box<wasi_config_t>> {
    debug_assert!(!program_name.is_null());

    let name_c_str = CStr::from_ptr(program_name);
    let prog_name = c_try!(name_c_str.to_str());

    Some(Box::new(wasi_config_t {
        inherit_stdout: true,
        inherit_stderr: true,
        inherit_stdin: true,
        state_builder: WasiState::new(prog_name),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasi_config_env(
    config: &mut wasi_config_t,
    key: *const c_char,
    value: *const c_char,
) {
    debug_assert!(!key.is_null());
    debug_assert!(!value.is_null());

    let key_cstr = CStr::from_ptr(key);
    let key_bytes = key_cstr.to_bytes();
    let value_cstr = CStr::from_ptr(value);
    let value_bytes = value_cstr.to_bytes();

    config.state_builder.env(key_bytes, value_bytes);
}

#[no_mangle]
pub unsafe extern "C" fn wasi_config_arg(config: &mut wasi_config_t, arg: *const c_char) {
    debug_assert!(!arg.is_null());

    let arg_cstr = CStr::from_ptr(arg);
    let arg_bytes = arg_cstr.to_bytes();

    config.state_builder.arg(arg_bytes);
}

#[no_mangle]
pub unsafe extern "C" fn wasi_config_preopen_dir(
    config: &mut wasi_config_t,
    dir: *const c_char,
) -> bool {
    let dir_cstr = CStr::from_ptr(dir);
    let dir_bytes = dir_cstr.to_bytes();
    let dir_str = match std::str::from_utf8(dir_bytes) {
        Ok(dir_str) => dir_str,
        Err(e) => {
            update_last_error(e);
            return false;
        }
    };

    if let Err(e) = config.state_builder.preopen_dir(dir_str) {
        update_last_error(e);
        return false;
    }

    true
}

#[no_mangle]
pub unsafe extern "C" fn wasi_config_mapdir(
    config: &mut wasi_config_t,
    alias: *const c_char,
    dir: *const c_char,
) -> bool {
    let alias_cstr = CStr::from_ptr(alias);
    let alias_bytes = alias_cstr.to_bytes();
    let alias_str = match std::str::from_utf8(alias_bytes) {
        Ok(alias_str) => alias_str,
        Err(e) => {
            update_last_error(e);
            return false;
        }
    };

    let dir_cstr = CStr::from_ptr(dir);
    let dir_bytes = dir_cstr.to_bytes();
    let dir_str = match std::str::from_utf8(dir_bytes) {
        Ok(dir_str) => dir_str,
        Err(e) => {
            update_last_error(e);
            return false;
        }
    };

    if let Err(e) = config.state_builder.map_dir(alias_str, dir_str) {
        update_last_error(e);
        return false;
    }

    true
}

#[no_mangle]
pub extern "C" fn wasi_config_capture_stdout(config: &mut wasi_config_t) {
    config.inherit_stdout = false;
}

#[no_mangle]
pub extern "C" fn wasi_config_inherit_stdout(config: &mut wasi_config_t) {
    config.inherit_stdout = true;
}

#[no_mangle]
pub extern "C" fn wasi_config_capture_stderr(config: &mut wasi_config_t) {
    config.inherit_stderr = false;
}

#[no_mangle]
pub extern "C" fn wasi_config_inherit_stderr(config: &mut wasi_config_t) {
    config.inherit_stderr = true;
}

//#[no_mangle]
//pub extern "C" fn wasi_config_capture_stdin(config: &mut wasi_config_t) {
//    config.inherit_stdin = false;
//}

#[no_mangle]
pub extern "C" fn wasi_config_inherit_stdin(config: &mut wasi_config_t) {
    config.inherit_stdin = true;
}

#[repr(C)]
pub struct wasi_filesystem_t {
    ptr: *const c_char,
    size: usize,
}

#[no_mangle]
pub unsafe extern "C" fn wasi_filesystem_init_static_memory(
    volume_bytes: Option<&wasm_byte_vec_t>,
) -> Option<Box<wasi_filesystem_t>> {
    let volume_bytes = volume_bytes.as_ref()?;
    Some(Box::new(wasi_filesystem_t {
        ptr: {
            let ptr = (volume_bytes.data.as_ref()?) as *const _ as *const c_char;
            if ptr.is_null() {
                return None;
            }
            ptr
        },
        size: volume_bytes.size,
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasi_filesystem_delete(ptr: *mut wasi_filesystem_t) {
    let _ = Box::from_raw(ptr);
}

/// Initializes the `imports` with an import object that links to
/// the custom file system
#[cfg(feature = "webc_runner")]
#[no_mangle]
pub unsafe extern "C" fn wasi_env_with_filesystem(
    config: Box<wasi_config_t>,
    store: Option<&mut wasm_store_t>,
    module: Option<&wasm_module_t>,
    fs: Option<&wasi_filesystem_t>,
    imports: Option<&mut wasm_extern_vec_t>,
    package: *const c_char,
) -> Option<Box<wasi_env_t>> {
    wasi_env_with_filesystem_inner(config, store, module, fs, imports, package)
}

#[cfg(feature = "webc_runner")]
unsafe fn wasi_env_with_filesystem_inner(
    config: Box<wasi_config_t>,
    store: Option<&mut wasm_store_t>,
    module: Option<&wasm_module_t>,
    fs: Option<&wasi_filesystem_t>,
    imports: Option<&mut wasm_extern_vec_t>,
    package: *const c_char,
) -> Option<Box<wasi_env_t>> {
    let store = &mut store?.inner;
    let fs = fs.as_ref()?;
    let package_str = CStr::from_ptr(package);
    let package = package_str.to_str().unwrap_or("");
    let module = &module.as_ref()?.inner;
    let imports = imports?;

    let (wasi_env, import_object) = prepare_webc_env(
        config,
        &mut store.store_mut(),
        module,
        std::mem::transmute(fs.ptr), // cast wasi_filesystem_t.ptr as &'static [u8]
        fs.size,
        package,
    )?;

    imports_set_buffer(&store, module, import_object, imports)?;

    Some(Box::new(wasi_env_t {
        inner: wasi_env,
        store: store.clone(),
    }))
}

#[cfg(feature = "webc_runner")]
fn prepare_webc_env(
    config: Box<wasi_config_t>,
    store: &mut impl AsStoreMut,
    module: &Module,
    bytes: &'static u8,
    len: usize,
    package_name: &str,
) -> Option<(WasiFunctionEnv, Imports)> {
    use wasmer_vfs::static_fs::StaticFileSystem;
    use webc::FsEntryType;

    let slice = unsafe { std::slice::from_raw_parts(bytes, len) };
    let volumes = webc::WebC::parse_volumes_from_fileblock(slice).ok()?;
    let top_level_dirs = volumes
        .into_iter()
        .flat_map(|(_, volume)| {
            volume
                .header
                .top_level
                .iter()
                .cloned()
                .filter(|e| e.fs_type == FsEntryType::Dir)
                .map(|e| e.text.to_string())
                .collect::<Vec<_>>()
                .into_iter()
        })
        .collect::<Vec<_>>();

    let filesystem = Box::new(StaticFileSystem::init(slice, &package_name)?);
    let mut wasi_env = config.state_builder;

    if !config.inherit_stdout {
        wasi_env.stdout(Box::new(Pipe::new()));
    }

    if !config.inherit_stderr {
        wasi_env.stderr(Box::new(Pipe::new()));
    }

    wasi_env.set_fs(filesystem);

    for f_name in top_level_dirs.iter() {
        wasi_env
            .preopen(|p| p.directory(f_name).read(true).write(true).create(true))
            .ok()?;
    }
    let env = wasi_env.finalize(store).ok()?;
    let import_object = env.import_object(store, &module).ok()?;
    Some((env, import_object))
}

#[allow(non_camel_case_types)]
pub struct wasi_env_t {
    /// cbindgen:ignore
    pub(super) inner: WasiFunctionEnv,
    pub(super) store: StoreRef,
}

/// Create a new WASI environment.
///
/// It take ownership over the `wasi_config_t`.
#[no_mangle]
pub unsafe extern "C" fn wasi_env_new(
    store: Option<&mut wasm_store_t>,
    mut config: Box<wasi_config_t>,
) -> Option<Box<wasi_env_t>> {
    let store = &mut store?.inner;
    let mut store_mut = store.store_mut();
    if !config.inherit_stdout {
        config.state_builder.stdout(Box::new(Pipe::new()));
    }

    if !config.inherit_stderr {
        config.state_builder.stderr(Box::new(Pipe::new()));
    }

    // TODO: impl capturer for stdin

    let wasi_state = c_try!(config.state_builder.finalize(&mut store_mut));

    Some(Box::new(wasi_env_t {
        inner: wasi_state,
        store: store.clone(),
    }))
}

/// Delete a [`wasi_env_t`].
#[no_mangle]
pub extern "C" fn wasi_env_delete(_state: Option<Box<wasi_env_t>>) {}

/// Set the memory on a [`wasi_env_t`].
#[no_mangle]
pub unsafe extern "C" fn wasi_env_set_memory(env: &mut wasi_env_t, memory: &wasm_memory_t) {
    let mut store_mut = env.store.store_mut();
    let wasi_env = env.inner.data_mut(&mut store_mut);
    wasi_env.set_memory(memory.extern_.memory());
}

#[no_mangle]
pub unsafe extern "C" fn wasi_env_read_stdout(
    env: &mut wasi_env_t,
    buffer: *mut c_char,
    buffer_len: usize,
) -> isize {
    let inner_buffer = slice::from_raw_parts_mut(buffer as *mut _, buffer_len as usize);
    let mut store_mut = env.store.store_mut();
    let state = env.inner.data_mut(&mut store_mut).state();

    if let Ok(mut stdout) = state.stdout() {
        if let Some(stdout) = stdout.as_mut() {
            read_inner(stdout, inner_buffer)
        } else {
            update_last_error("could not find a file handle for `stdout`");
            -1
        }
    } else {
        update_last_error("could not find a file handle for `stdout`");
        -1
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasi_env_read_stderr(
    env: &mut wasi_env_t,
    buffer: *mut c_char,
    buffer_len: usize,
) -> isize {
    let inner_buffer = slice::from_raw_parts_mut(buffer as *mut _, buffer_len as usize);
    let mut store_mut = env.store.store_mut();
    let state = env.inner.data_mut(&mut store_mut).state();
    if let Ok(mut stderr) = state.stderr() {
        if let Some(stderr) = stderr.as_mut() {
            read_inner(stderr, inner_buffer)
        } else {
            update_last_error("could not find a file handle for `stderr`");
            -1
        }
    } else {
        update_last_error("could not find a file handle for `stderr`");
        -1
    }
}

fn read_inner(
    wasi_file: &mut Box<dyn WasiFile + Send + Sync + 'static>,
    inner_buffer: &mut [u8],
) -> isize {
    match wasi_file.read(inner_buffer) {
        Ok(a) => a as isize,
        Err(err) => {
            update_last_error(format!("failed to read wasi_file: {}", err));
            -1
        }
    }
}

/// The version of WASI. This is determined by the imports namespace
/// string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
#[allow(non_camel_case_types)]
pub enum wasi_version_t {
    /// An invalid version.
    INVALID_VERSION = -1,

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
    LATEST = 0,

    /// `wasi_unstable`.
    SNAPSHOT0 = 1,

    /// `wasi_snapshot_preview1`.
    SNAPSHOT1 = 2,

    /// `wasix_32v1`.
    WASIX32V1 = 3,

    /// `wasix_64v1`.
    WASIX64V1 = 4,
}

impl From<WasiVersion> for wasi_version_t {
    fn from(other: WasiVersion) -> Self {
        match other {
            WasiVersion::Snapshot0 => wasi_version_t::SNAPSHOT0,
            WasiVersion::Snapshot1 => wasi_version_t::SNAPSHOT1,
            WasiVersion::Wasix32v1 => wasi_version_t::WASIX32V1,
            WasiVersion::Wasix64v1 => wasi_version_t::WASIX64V1,
            WasiVersion::Latest => wasi_version_t::LATEST,
        }
    }
}

impl TryFrom<wasi_version_t> for WasiVersion {
    type Error = &'static str;

    fn try_from(other: wasi_version_t) -> Result<Self, Self::Error> {
        Ok(match other {
            wasi_version_t::INVALID_VERSION => return Err("Invalid WASI version cannot be used"),
            wasi_version_t::SNAPSHOT0 => WasiVersion::Snapshot0,
            wasi_version_t::SNAPSHOT1 => WasiVersion::Snapshot1,
            wasi_version_t::WASIX32V1 => WasiVersion::Wasix32v1,
            wasi_version_t::WASIX64V1 => WasiVersion::Wasix64v1,
            wasi_version_t::LATEST => WasiVersion::Latest,
        })
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasi_get_wasi_version(module: &wasm_module_t) -> wasi_version_t {
    get_wasi_version(&module.inner, false)
        .map(Into::into)
        .unwrap_or(wasi_version_t::INVALID_VERSION)
}

/// Non-standard function to get the imports needed for the WASI
/// implementation ordered as expected by the `wasm_module_t`.
#[no_mangle]
pub unsafe extern "C" fn wasi_get_imports(
    _store: Option<&wasm_store_t>,
    wasi_env: Option<&mut wasi_env_t>,
    module: Option<&wasm_module_t>,
    imports: &mut wasm_extern_vec_t,
) -> bool {
    wasi_get_imports_inner(wasi_env, module, imports).is_some()
}

unsafe fn wasi_get_imports_inner(
    wasi_env: Option<&mut wasi_env_t>,
    module: Option<&wasm_module_t>,
    imports: &mut wasm_extern_vec_t,
) -> Option<()> {
    let wasi_env = wasi_env?;
    let store = &mut wasi_env.store;
    let mut store_mut = store.store_mut();
    let module = module?;

    let import_object = c_try!(wasi_env.inner.import_object(&mut store_mut, &module.inner));

    imports_set_buffer(store, &module.inner, import_object, imports)?;

    Some(())
}

pub(crate) fn imports_set_buffer(
    store: &StoreRef,
    module: &wasmer_api::Module,
    import_object: wasmer_api::Imports,
    imports: &mut wasm_extern_vec_t,
) -> Option<()> {
    imports.set_buffer(c_try!(module
        .imports()
        .map(|import_type| {
            let ext = import_object
                .get_export(import_type.module(), import_type.name())
                .ok_or_else(|| {
                    format!(
                        "Failed to resolve import \"{}\" \"{}\"",
                        import_type.module(),
                        import_type.name()
                    )
                })?;

            Ok(Some(Box::new(wasm_extern_t::new(store.clone(), ext))))
        })
        .collect::<Result<Vec<_>, String>>()));

    Some(())
}

#[no_mangle]
pub unsafe extern "C" fn wasi_env_initialize_instance(
    wasi_env: &mut wasi_env_t,
    store: &mut wasm_store_t,
    instance: &mut wasm_instance_t,
) -> bool {
    let mem = c_try!(instance.inner.exports.get_memory("memory"); otherwise false);
    wasi_env
        .inner
        .data_mut(&mut store.inner.store_mut())
        .set_memory(mem.clone());
    true
}

#[no_mangle]
pub unsafe extern "C" fn wasi_get_start_function(
    instance: &mut wasm_instance_t,
) -> Option<Box<wasm_func_t>> {
    let start = c_try!(instance.inner.exports.get_function("_start"));

    Some(Box::new(wasm_func_t {
        extern_: wasm_extern_t::new(instance.store.clone(), start.clone().into()),
    }))
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "windows"))]
    use inline_c::assert_c;
    #[cfg(target_os = "windows")]
    use wasmer_inline_c::assert_c;

    #[test]
    fn test_wasi_get_wasi_version_snapshot0() {
        (assert_c! {
            #include "tests/wasmer.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);
                wasmer_funcenv_t* env = wasmer_funcenv_new(store, 0);

                wasm_byte_vec_t wat;
                wasmer_byte_vec_new_from_string(&wat, "(module (import \"wasi_unstable\" \"args_get\" (func (param i32 i32) (result i32))))");
                wasm_byte_vec_t wasm;
                wat2wasm(&wat, &wasm);

                wasm_module_t* module = wasm_module_new(store, &wasm);
                assert(module);

                assert(wasi_get_wasi_version(module) == SNAPSHOT0);

                wasm_module_delete(module);
                wasm_byte_vec_delete(&wasm);
                wasm_byte_vec_delete(&wat);
                wasmer_funcenv_delete(env);
                wasm_store_delete(store);
                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }

    #[test]
    fn test_wasi_get_wasi_version_snapshot1() {
        (assert_c! {
            #include "tests/wasmer.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);
                wasmer_funcenv_t* env = wasmer_funcenv_new(store, 0);

                wasm_byte_vec_t wat;
                wasmer_byte_vec_new_from_string(&wat, "(module (import \"wasi_snapshot_preview1\" \"args_get\" (func (param i32 i32) (result i32))))");
                wasm_byte_vec_t wasm;
                wat2wasm(&wat, &wasm);

                wasm_module_t* module = wasm_module_new(store, &wasm);
                assert(module);

                assert(wasi_get_wasi_version(module) == SNAPSHOT1);

                wasm_module_delete(module);
                wasm_byte_vec_delete(&wasm);
                wasm_byte_vec_delete(&wat);
                wasmer_funcenv_delete(env);
                wasm_store_delete(store);
                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }

    #[test]
    fn test_wasi_get_wasi_version_invalid() {
        (assert_c! {
            #include "tests/wasmer.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);
                wasmer_funcenv_t* env = wasmer_funcenv_new(store, 0);

                wasm_byte_vec_t wat;
                wasmer_byte_vec_new_from_string(&wat, "(module (import \"wasi_snpsht_prvw1\" \"args_get\" (func (param i32 i32) (result i32))))");
                wasm_byte_vec_t wasm;
                wat2wasm(&wat, &wasm);

                wasm_module_t* module = wasm_module_new(store, &wasm);
                assert(module);

                assert(wasi_get_wasi_version(module) == INVALID_VERSION);

                wasm_module_delete(module);
                wasm_byte_vec_delete(&wasm);
                wasm_byte_vec_delete(&wat);
                wasmer_funcenv_delete(env);
                wasm_store_delete(store);
                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }
}
