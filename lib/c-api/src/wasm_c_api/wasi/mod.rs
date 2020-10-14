//! Unofficial API for WASI integrating with the standard Wasm C API.
//!
//! This API will be superseded by a standard WASI API when/if such a standard is created.

mod capture_files;

use super::{
    externals::{wasm_extern_t, wasm_func_t, wasm_memory_t},
    instance::wasm_instance_t,
    module::wasm_module_t,
    store::wasm_store_t,
};
// required due to really weird Rust resolution rules for macros
// https://github.com/rust-lang/rust/issues/57966
use crate::error::{update_last_error, CApiError};
use std::convert::TryFrom;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::slice;
use wasmer::{Extern, NamedResolver};
use wasmer_wasi::{
    generate_import_object_from_env, get_wasi_version, WasiEnv, WasiFile, WasiState,
    WasiStateBuilder, WasiVersion,
};

#[derive(Debug, Default)]
#[allow(non_camel_case_types)]
pub struct wasi_config_t {
    inherit_stdout: bool,
    inherit_stderr: bool,
    inherit_stdin: bool,
    /// cbindgen:ignore
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
        state_builder: WasiState::new(prog_name),
        ..wasi_config_t::default()
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
pub extern "C" fn wasi_config_inherit_stdout(config: &mut wasi_config_t) {
    config.inherit_stdout = true;
}
#[no_mangle]
pub extern "C" fn wasi_config_inherit_stderr(config: &mut wasi_config_t) {
    config.inherit_stderr = true;
}
#[no_mangle]
pub extern "C" fn wasi_config_inherit_stdin(config: &mut wasi_config_t) {
    config.inherit_stdin = true;
}

#[allow(non_camel_case_types)]
pub struct wasi_env_t {
    /// cbindgen:ignore
    inner: WasiEnv,
}

/// Takes ownership over the `wasi_config_t`.
#[no_mangle]
pub extern "C" fn wasi_env_new(mut config: Box<wasi_config_t>) -> Option<Box<wasi_env_t>> {
    if config.inherit_stdout {
        config
            .state_builder
            .stdout(Box::new(capture_files::OutputCapturer::new()));
    }
    if config.inherit_stderr {
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

#[no_mangle]
pub extern "C" fn wasi_env_delete(_state: Option<Box<wasi_env_t>>) {}

#[no_mangle]
pub extern "C" fn wasi_env_set_instance(env: &mut wasi_env_t, instance: &wasm_instance_t) -> bool {
    let memory = if let Ok(memory) = instance.inner.exports.get_memory("memory") {
        memory
    } else {
        return false;
    };
    env.inner.set_memory(memory.clone());

    true
}

#[no_mangle]
pub extern "C" fn wasi_env_set_memory(env: &mut wasi_env_t, memory: &wasm_memory_t) {
    env.inner.set_memory(memory.inner.clone());
}

#[no_mangle]
pub unsafe extern "C" fn wasi_env_read_stdout(
    env: &mut wasi_env_t,
    buffer: *mut c_char,
    buffer_len: usize,
) -> isize {
    let inner_buffer = slice::from_raw_parts_mut(buffer as *mut _, buffer_len as usize);
    let mut state = env.inner.state_mut();

    let stdout = if let Ok(stdout) = state.fs.stdout_mut() {
        if let Some(stdout) = stdout.as_mut() {
            stdout
        } else {
            update_last_error(CApiError {
                msg: "could not find a file handle for `stdout`".to_string(),
            });
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
    let mut state = env.inner.state_mut();
    let stderr = if let Ok(stderr) = state.fs.stderr_mut() {
        if let Some(stderr) = stderr.as_mut() {
            stderr
        } else {
            update_last_error(CApiError {
                msg: "could not find a file handle for `stderr`".to_string(),
            });
            return -1;
        }
    } else {
        update_last_error(CApiError {
            msg: "could not find a file handle for `stderr`".to_string(),
        });
        return -1;
    };
    read_inner(stderr, inner_buffer)
}

fn read_inner(wasi_file: &mut Box<dyn WasiFile>, inner_buffer: &mut [u8]) -> isize {
    if let Some(oc) = wasi_file.downcast_mut::<capture_files::OutputCapturer>() {
        let mut num_bytes_written = 0;
        for (address, value) in inner_buffer.iter_mut().zip(oc.buffer.drain(..)) {
            *address = value;
            num_bytes_written += 1;
        }
        num_bytes_written
    } else {
        -1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
#[allow(non_camel_case_types)]
pub enum wasi_version_t {
    Latest = 0,
    Snapshot0 = 1,
    Snapshot1 = 2,
    InvalidVersion = u32::max_value(),
}

impl From<WasiVersion> for wasi_version_t {
    fn from(other: WasiVersion) -> Self {
        match other {
            WasiVersion::Snapshot0 => wasi_version_t::Snapshot0,
            WasiVersion::Snapshot1 => wasi_version_t::Snapshot1,
            WasiVersion::Latest => wasi_version_t::Latest,
        }
    }
}

impl TryFrom<wasi_version_t> for WasiVersion {
    type Error = &'static str;

    fn try_from(other: wasi_version_t) -> Result<Self, Self::Error> {
        Ok(match other {
            wasi_version_t::Snapshot0 => WasiVersion::Snapshot0,
            wasi_version_t::Snapshot1 => WasiVersion::Snapshot1,
            wasi_version_t::Latest => WasiVersion::Latest,
            wasi_version_t::InvalidVersion => return Err("Invalid WASI version cannot be used"),
        })
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasi_get_wasi_version(module: &wasm_module_t) -> wasi_version_t {
    get_wasi_version(&module.inner, false)
        .map(Into::into)
        .unwrap_or(wasi_version_t::InvalidVersion)
}

/// Takes ownership of `wasi_env_t`.
#[no_mangle]
pub unsafe extern "C" fn wasi_get_imports(
    store: &wasm_store_t,
    module: &wasm_module_t,
    wasi_env: &wasi_env_t,
    imports: *mut *mut wasm_extern_t,
) -> bool {
    wasi_get_imports_inner(store, module, wasi_env, imports).is_some()
}

/// Takes ownership of `wasi_env_t`.
unsafe fn wasi_get_imports_inner(
    store: &wasm_store_t,
    module: &wasm_module_t,
    wasi_env: &wasi_env_t,
    imports: *mut *mut wasm_extern_t,
) -> Option<()> {
    let store = &store.inner;

    let version = c_try!(
        get_wasi_version(&module.inner, false).ok_or_else(|| CApiError {
            msg: "could not detect a WASI version on the given module".to_string(),
        })
    );

    let import_object = generate_import_object_from_env(store, wasi_env.inner.clone(), version);

    for (i, it) in module.inner.imports().enumerate() {
        let export = c_try!(import_object
            .resolve_by_name(it.module(), it.name())
            .ok_or_else(|| CApiError {
                msg: format!(
                    "Failed to resolve import \"{}\" \"{}\"",
                    it.module(),
                    it.name()
                ),
            }));
        let inner = Extern::from_export(store, export);
        *imports.add(i) = Box::into_raw(Box::new(wasm_extern_t {
            instance: None,
            inner,
        }));
    }

    Some(())
}

#[no_mangle]
pub unsafe extern "C" fn wasi_get_start_function(
    instance: &mut wasm_instance_t,
) -> Option<Box<wasm_func_t>> {
    let f = c_try!(instance.inner.exports.get_function("_start"));
    Some(Box::new(wasm_func_t {
        inner: f.clone(),
        instance: Some(instance.inner.clone()),
    }))
}

/// Delete a `wasm_extern_t` allocated by the API.
///
/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_extern_delete(_item: Option<Box<wasm_extern_t>>) {}
