//! Unofficial API for WASI integrating with the standard Wasm C API.
//!
//! This API will be superseded by a standard WASI API when/if such a standard is created.

pub use super::unstable::wasi::wasi_get_unordered_imports;
use super::{
    externals::{wasm_extern_t, wasm_extern_vec_t, wasm_func_t},
    instance::wasm_instance_t,
    module::wasm_module_t,
    store::{wasm_store_t, StoreRef},
};
use crate::error::update_last_error;
use std::{io::{self, SeekFrom}, fmt, convert::TryFrom, sync::{atomic::{AtomicBool, Ordering}, MutexGuard}};
use std::ffi::CStr;
use std::convert::TryInto;
use std::sync::{Mutex, Arc};
use std::os::raw::c_char;
use std::slice;
use wasmer_wasi::{
    get_wasi_version, VirtualFile, FsError,
     WasiFile, WasiFunctionEnv, WasiState, 
     WasiStateBuilder, WasiVersion,
};

/// Function callback that takes:
/// 
/// - a *mut to the environment data (passed in on creation), 
/// - the length of the environment data
/// - a *const to the bytes to write
/// - the length of the bytes to write 
pub type WasiConsoleIoReadCallback = unsafe extern "C" fn(*mut c_char, usize, *mut c_char, usize) -> i64;
pub type WasiConsoleIoWriteCallback = unsafe extern "C" fn(*mut c_char, usize, *const c_char, usize, bool) -> i64;
pub type WasiConsoleIoSeekCallback = unsafe extern "C" fn(*mut c_char, usize, c_char, i64) -> i64;
pub type WasiConsoleIoEnvDestructor = unsafe extern "C" fn (*mut c_char, usize) -> i64;

/// The console override is a custom context consisting of callback pointers 
/// (which are activated whenever some console I/O occurs) and a "context", which
/// can be owned or referenced from C. This struct can be used in `wasi_config_overwrite_stdin`, 
/// `wasi_config_overwrite_stdout` or `wasi_config_overwrite_stderr` to redirect the output or
/// insert input into the console I/O log.
/// 
/// Internally the stdout / stdin is synchronized, so the console is usable across threads 
/// (only one thread can read / write / seek from the console I/O)
#[allow(non_camel_case_types)]
#[repr(C)]
pub struct wasi_console_io_override_t {
    read: WasiConsoleIoReadCallback,
    write: WasiConsoleIoWriteCallback,
    seek: WasiConsoleIoSeekCallback,
    destructor: WasiConsoleIoEnvDestructor,
    data: Option<Arc<Mutex<Vec<c_char>>>>,
    dropped: AtomicBool,
}

impl wasi_console_io_override_t {
    fn get_data_mut(&mut self, op_id: &'static str) -> io::Result<MutexGuard<Vec<c_char>>> {
        self.data
        .as_mut()
        .ok_or({
            io::Error::new(io::ErrorKind::Other, format!("could not lock mutex ({op_id}) on wasi_console_io_override_t: no mutex"))
        })?
        .lock()
        .map_err(|e| {
            io::Error::new(io::ErrorKind::Other, format!("could not lock mutex ({op_id}) on wasi_console_io_override_t: {e}"))
        })
    }
}

impl Drop for wasi_console_io_override_t {
    fn drop(&mut self) {

        let data = match self.data.take() {
            Some(s) => s,
            None => { return; },
        };

        let value = match Arc::try_unwrap(data) {
            Ok(o) => o,
            Err(_) => { return; },
        };

        let mut inner_value = match value.into_inner() {
            Ok(o) => o,
            Err(_) => { return; },
        };

        if self.dropped.load(Ordering::SeqCst) {
            return;
        }

        let error = unsafe { (self.destructor)(inner_value.as_mut_ptr(), inner_value.len()) };
        if error <= 0 {
            println!("error dropping wasi_console_io_override_t: {error}");
        }

        self.dropped.store(true, Ordering::SeqCst);
    }
}

impl fmt::Debug for wasi_console_io_override_t {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "wasi_console_io_override_t")
    }
}

impl io::Read for wasi_console_io_override_t {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let self_read = self.read.clone();
        let mut data = self.get_data_mut("read")?;
        let result = unsafe { (self_read)(data.as_mut_ptr(), data.len(), buf.as_mut_ptr() as *mut c_char, buf.len()) };
        if result >= 0 {
            Ok(result as usize)
        } else {
            Err(io::Error::new(io::ErrorKind::Other, format!("could not read from wasi_console_io_override_t: {result}")))
        }
    }
}

impl io::Write for wasi_console_io_override_t {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let self_write = self.write.clone();
        let mut data = self.get_data_mut("write")?;
        let result = unsafe { (self_write)(data.as_mut_ptr(), data.len(), buf.as_ptr() as *const c_char, buf.len(), false) };
        if result >= 0 {
            Ok(result.try_into().unwrap_or(0))
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::Other, format!("could not write {} bytes to wasi_console_io_override_t: {result}", buf.len())))
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        let self_write = self.write.clone();
        let mut data = self.get_data_mut("flush")?;
        let bytes_to_write = &[];
        let result: i64 = unsafe { (self_write)(data.as_mut_ptr(), data.len(), bytes_to_write.as_ptr(), 0, true) };
        if result >= 0 {
            Ok(())
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::Other, format!("could not flush wasi_console_io_override_t: {result}")))
        }
    }
}

impl io::Seek for wasi_console_io_override_t {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let self_seek = self.seek.clone();
        let mut data = self.get_data_mut("seek")?;
        let (id, pos) = match pos {
            SeekFrom::Start(s) => (0, s as i64),
            SeekFrom::End(s) => (1, s),
            SeekFrom::Current(s) => (2, s),
        };
        let result = unsafe { (self_seek)(data.as_mut_ptr(), data.len(), id, pos) };
        if result >= 0 {
            Ok(result.try_into().unwrap_or(0))
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::Other, format!("could not seek to {pos:?} wasi_console_io_override_t: {result}")))
        }
    }
}

impl VirtualFile for wasi_console_io_override_t {
    fn last_accessed(&self) -> u64 { 0 }
    fn last_modified(&self) -> u64 { 0 }
    fn created_time(&self) -> u64 { 0 }
    fn size(&self) -> u64 { 0 }
    fn set_len(&mut self, _: u64) -> Result<(), FsError> { Ok(()) }
    fn unlink(&mut self) -> Result<(), FsError> { Ok(()) }
}

/// Creates a new callback object that is being
#[no_mangle]
pub unsafe extern "C" fn wasi_console_io_override_new(
    read: WasiConsoleIoReadCallback,
    write: WasiConsoleIoWriteCallback,
    seek: WasiConsoleIoSeekCallback,
    destructor: WasiConsoleIoEnvDestructor,
    env_data: *mut c_char,
    env_data_len: usize,
    transfer_ownership: bool,
) -> *mut wasi_console_io_override_t {

    let data_vec = if transfer_ownership {
        std::slice::from_raw_parts(env_data, env_data_len).to_vec()
    } else {
        Vec::from_raw_parts(env_data, env_data_len, env_data_len)
    };

    Box::leak(Box::new(wasi_console_io_override_t {
        read,
        write,
        seek,
        destructor,
        data: Some(Arc::new(Mutex::new(data_vec))),
        dropped: AtomicBool::new(false),
    }))
}

/// Creates a `wasi_console_io_override_t` callback object that does nothing
/// and redirects stdout / stderr to /dev/null
#[no_mangle]
pub unsafe extern "C" fn wasi_console_override_new_null() -> *mut wasi_console_io_override_t {
    let mut data = Vec::new();
    wasi_console_io_override_new(
        wasi_console_io_override_read, 
        wasi_console_io_override_write, 
        wasi_console_io_override_seek, 
        wasi_console_io_override_delete, 
        data.as_mut_ptr(), 
        data.len(), 
        true
    )
}

extern "C" fn wasi_console_io_override_read(_: *mut c_char, _:usize, _:*mut c_char, _: usize) -> i64 { 0 }
extern "C" fn wasi_console_io_override_write(_: *mut c_char, _: usize, _: *const c_char, _: usize, _: bool) -> i64 { 0 }
extern "C" fn wasi_console_io_override_seek(_: *mut c_char, _: usize, _: c_char, _: i64) -> i64 { 0 }
extern "C" fn wasi_console_io_override_delete(_: *mut c_char, _: usize) -> i64 { 0 }

#[derive(Debug)]
#[allow(non_camel_case_types)]
pub struct wasi_config_t {
    inherit_stdout: Option<Box<wasi_console_io_override_t>>,
    inherit_stderr: Option<Box<wasi_console_io_override_t>>,
    inherit_stdin: Option<Box<wasi_console_io_override_t>>,
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
        inherit_stdout: None,
        inherit_stderr: None,
        inherit_stdin: None,
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
    config.inherit_stdout = Some(unsafe { Box::from_raw(wasi_console_override_new_null()) });
}

#[no_mangle]
pub extern "C" fn wasi_config_inherit_stdout(config: &mut wasi_config_t) {
    config.inherit_stdout = None;
}

#[no_mangle]
pub extern "C" fn wasi_config_capture_stderr(config: &mut wasi_config_t) {
    config.inherit_stderr = Some(unsafe { Box::from_raw(wasi_console_override_new_null()) });
}

#[no_mangle]
pub extern "C" fn wasi_config_inherit_stderr(config: &mut wasi_config_t) {
    config.inherit_stderr = None;
}

#[no_mangle]
pub extern "C" fn wasi_config_capture_stdin(config: &mut wasi_config_t) {
    config.inherit_stdin = Some(unsafe { Box::from_raw(wasi_console_override_new_null()) });
}

#[no_mangle]
pub extern "C" fn wasi_config_inherit_stdin(config: &mut wasi_config_t) {
    config.inherit_stdin = None;
}

#[no_mangle]
pub unsafe extern "C" fn wasi_config_overwrite_stdin(
    config: &mut wasi_config_t, 
    stdin: *mut wasi_console_io_override_t
) {
    config.state_builder.stdin(Box::from_raw(stdin));
}

#[no_mangle]
pub unsafe extern "C" fn wasi_config_overwrite_stdout(
    config: &mut wasi_config_t, 
    stdout: *mut wasi_console_io_override_t
) {
    config.state_builder.stdout(Box::from_raw(stdout));
}

#[no_mangle]
pub unsafe extern "C" fn wasi_config_overwrite_stderr(
    config: &mut wasi_config_t, 
    stderr: *mut wasi_console_io_override_t
) {
    config.state_builder.stderr(Box::from_raw(stderr));
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

    if let Some(stdout) = config.inherit_stdout {
        config.state_builder.stdout(stdout);
    }

    if let Some(stderr) = config.inherit_stderr {
        config.state_builder.stderr(stderr);
    }

    if let Some(stdin) = config.inherit_stdin {
        config.state_builder.stdin(stdin);
    }

    let wasi_state = c_try!(config.state_builder.finalize(&mut store_mut));

    Some(Box::new(wasi_env_t {
        inner: wasi_state,
        store: store.clone(),
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

#[no_mangle]
pub unsafe extern "C" fn wasi_env_write_stdin(
    env: &mut wasi_env_t,
    buffer: *const u8,
    buffer_len: usize,
) -> bool {
    let mut store_mut = env.store.store_mut();
    let state = env.inner.data_mut(&mut store_mut).state();
    let mut stdin =
        c_try!(state.stdin(); otherwise false).ok_or("Could not access WASI's state stdin");
    let wasi_stdin = c_try!(stdin.as_mut(); otherwise false);
    let buffer = slice::from_raw_parts(buffer, buffer_len);
    let msg = c_try!(std::str::from_utf8(buffer); otherwise false);
    c_try!(write!(wasi_stdin, "{}", msg); otherwise false);
    true
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

    imports.set_buffer(c_try!(module
        .inner
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
    use inline_c::assert_c;

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

    #[test]
    fn test_wasi_stdin_set() {
        (assert_c! {
            #include "tests/wasmer.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);
                wasi_config_t* config = wasi_config_new("example_program");
                wasi_config_capture_stdout(config);
                wasi_config_overwrite_stdin(config);

                wasm_byte_vec_t wat;
                wasmer_byte_vec_new_from_string(&wat, "(module (import \"wasi_unstable\" \"args_get\" (func (param i32 i32) (result i32))))");
                wasm_byte_vec_t wasm;
                wat2wasm(&wat, &wasm);

                wasm_module_t* module = wasm_module_new(store, &wasm);
                assert(module);

                // TODO FIXME
                //
                // Test captured stdin

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
