//! Unofficial API for WASI integrating with the standard Wasm C API.
//!
//! This API will be superseded by a standard WASI API when/if such a standard is created.

mod capture_files;

pub use super::unstable::wasi::wasi_get_unordered_imports;
use super::{
    externals::{wasm_extern_vec_t, wasm_func_t},
    instance::wasm_instance_t,
    module::wasm_module_t,
    store::wasm_store_t,
};
use crate::error::update_last_error;
use std::cmp::min;
use std::convert::TryFrom;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::slice;
use wasmer_api::{Extern, NamedResolver};
use wasmer_wasi::{
    generate_import_object_from_env, get_wasi_version, WasiEnv, WasiFile, WasiState,
    WasiStateBuilder, WasiVersion,
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

#[allow(non_camel_case_types)]
pub struct wasi_env_t {
    /// cbindgen:ignore
    pub(super) inner: WasiEnv,
}

/// Create a new WASI environment.
///
/// It take ownership over the `wasi_config_t`.
#[no_mangle]
pub extern "C" fn wasi_env_new(mut config: Box<wasi_config_t>) -> Option<Box<wasi_env_t>> {
    if !config.inherit_stdout {
        config
            .state_builder
            .stdout(Box::new(capture_files::OutputCapturer::new()));
    }

    if !config.inherit_stderr {
        config
            .state_builder
            .stderr(Box::new(capture_files::OutputCapturer::new()));
    }

    // TODO: impl capturer for stdin

    let wasi_state = c_try!(config.state_builder.build());

    Some(Box::new(wasi_env_t {
        inner: WasiEnv::new(wasi_state),
    }))
}

/// Delete a [`wasi_env_t`].
#[no_mangle]
pub extern "C" fn wasi_env_delete(_state: Option<Box<wasi_env_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasi_env_read_stdout(
    env: &mut wasi_env_t,
    buffer: *mut c_char,
    buffer_len: usize,
) -> isize {
    let inner_buffer = slice::from_raw_parts_mut(buffer as *mut _, buffer_len as usize);
    let mut state = env.inner.state();

    let stdout = if let Ok(stdout) = state.fs.stdout_mut() {
        if let Some(stdout) = stdout.as_mut() {
            stdout
        } else {
            update_last_error("could not find a file handle for `stdout`");
            return -1;
        }
    } else {
        return -1;
    };
    read_inner(stdout, inner_buffer)
}

#[no_mangle]
pub unsafe extern "C" fn wasi_env_read_stderr(
    env: &mut wasi_env_t,
    buffer: *mut c_char,
    buffer_len: usize,
) -> isize {
    let inner_buffer = slice::from_raw_parts_mut(buffer as *mut _, buffer_len as usize);
    let mut state = env.inner.state();
    let stderr = if let Ok(stderr) = state.fs.stderr_mut() {
        if let Some(stderr) = stderr.as_mut() {
            stderr
        } else {
            update_last_error("could not find a file handle for `stderr`");
            return -1;
        }
    } else {
        update_last_error("could not find a file handle for `stderr`");
        return -1;
    };
    read_inner(stderr, inner_buffer)
}

fn read_inner(wasi_file: &mut Box<dyn WasiFile>, inner_buffer: &mut [u8]) -> isize {
    if let Some(oc) = wasi_file.downcast_mut::<capture_files::OutputCapturer>() {
        let total_to_read = min(inner_buffer.len(), oc.buffer.len());

        for (address, value) in inner_buffer
            .iter_mut()
            .zip(oc.buffer.drain(..total_to_read))
        {
            *address = value;
        }

        total_to_read as isize
    } else {
        -1
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
}

impl From<WasiVersion> for wasi_version_t {
    fn from(other: WasiVersion) -> Self {
        match other {
            WasiVersion::Snapshot0 => wasi_version_t::SNAPSHOT0,
            WasiVersion::Snapshot1 => wasi_version_t::SNAPSHOT1,
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
    store: Option<&wasm_store_t>,
    module: Option<&wasm_module_t>,
    wasi_env: Option<&wasi_env_t>,
    imports: &mut wasm_extern_vec_t,
) -> bool {
    wasi_get_imports_inner(store, module, wasi_env, imports).is_some()
}

fn wasi_get_imports_inner(
    store: Option<&wasm_store_t>,
    module: Option<&wasm_module_t>,
    wasi_env: Option<&wasi_env_t>,
    imports: &mut wasm_extern_vec_t,
) -> Option<()> {
    let store = store?;
    let module = module?;
    let wasi_env = wasi_env?;

    let store = &store.inner;

    let version = c_try!(get_wasi_version(&module.inner, false)
        .ok_or("could not detect a WASI version on the given module"));

    let import_object = generate_import_object_from_env(store, wasi_env.inner.clone(), version);

    imports.set_buffer(c_try!(module
        .inner
        .imports()
        .map(|import_type| {
            let export = import_object
                .resolve_by_name(import_type.module(), import_type.name())
                .ok_or_else(|| {
                    format!(
                        "Failed to resolve import \"{}\" \"{}\"",
                        import_type.module(),
                        import_type.name()
                    )
                })?;
            let inner = Extern::from_vm_export(store, export);

            Ok(Some(Box::new(inner.into())))
        })
        .collect::<Result<Vec<_>, String>>()));

    Some(())
}

#[no_mangle]
pub unsafe extern "C" fn wasi_get_start_function(
    instance: &mut wasm_instance_t,
) -> Option<Box<wasm_func_t>> {
    let start = c_try!(instance.inner.exports.get_function("_start"));

    Some(Box::new(wasm_func_t::new(start.clone())))
}

#[cfg(test)]
mod tests {
    use inline_c::assert_c;

    #[test]
    fn test_wasi_get_wasi_version_snapshot0() {
        (assert_c! {
            #include "tests/wasmer.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);

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
                wasm_store_delete(store);
                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }
}
