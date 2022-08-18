//! Unofficial API for WASI integrating with the standard Wasm C API.
//!
//! This API will be superseded by a standard WASI API when/if such a standard is created.

pub use super::unstable::wasi::wasi_get_unordered_imports;
use super::{
    externals::{wasm_extern_t, wasm_extern_vec_t, wasm_func_t, wasm_memory_t},
    instance::wasm_instance_t,
    module::wasm_module_t,
    store::{wasm_store_t, StoreRef},
};
use crate::error::update_last_error;
use std::convert::TryInto;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::slice;
use std::sync::{Arc, Mutex};
use std::{
    convert::TryFrom,
    ffi::c_void,
    fmt,
    io::{self, SeekFrom},
    sync::MutexGuard,
};
use wasmer_wasi::{
    get_wasi_version, FsError, VirtualFile, WasiFile, WasiFunctionEnv, WasiState, WasiStateBuilder,
    WasiVersion, Pipe,
};

/// Function callback that takes:
///
/// - a *mut to the environment data (passed in on creation),
/// - the length of the environment data
/// - a *const to the bytes to write
/// - the length of the bytes to write
pub type WasiConsoleIoReadCallback =
    unsafe extern "C" fn(*const c_void, *mut c_char, usize) -> i64;
pub type WasiConsoleIoWriteCallback =
    unsafe extern "C" fn(*const c_void, *const c_char, usize, bool) -> i64;
pub type WasiConsoleIoSeekCallback =
    unsafe extern "C" fn(*const c_void, c_char, i64) -> i64;
pub type WasiConsoleIoEnvDestructor = unsafe extern "C" fn(*const c_void) -> i64;
/// Callback that is activated whenever the program wants to read from stdin
///
/// Parameters:
///     - `void*`: to user-defined data
///     - `usize`: sizeof(user-defined data)
///     - `usize`: alignof(user-defined data)
///     - `usize`: maximum bytes that can be written to stdin
///     - `*mut wasi_console_stdin_response_t`: handle to the stdin response, used to write data to stdin
///
/// The function returning is the same as the program receiving an "enter"
/// key event from the console I/O. With the custom environment pointer (the first argument)
/// you can clone references to the stdout channel and - for example - inspect the stdout
/// channel to answer depending on runtime-dependent stdout data.
pub type WasiConsoleIoOnStdinCallback = unsafe extern "C" fn(
    *const c_void,
    usize,
    *mut wasi_console_stdin_response_t,
) -> i64;

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
#[derive(Clone)]
pub struct wasi_console_out_t {
    read: WasiConsoleIoReadCallback,
    write: WasiConsoleIoWriteCallback,
    seek: WasiConsoleIoSeekCallback,
    destructor: WasiConsoleIoEnvDestructor,
    data: Option<Box<Arc<Mutex<Vec<c_char>>>>>,
}

impl wasi_console_out_t {
    fn get_data_mut(&self, op_id: &'static str) -> io::Result<MutexGuard<Vec<c_char>>> {
        self.data
            .as_ref()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("could not lock mutex ({op_id}) on wasi_console_out_t: no mutex"),
                )
            })?
            .lock()
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("could not lock mutex ({op_id}) on wasi_console_out_t: {e}"),
                )
            })
    }
}

impl Drop for wasi_console_out_t {
    fn drop(&mut self) {
        let data = match self.data.take() {
            Some(s) => s,
            None => {
                return;
            }
        };

        let value = match Arc::try_unwrap(*data) {
            Ok(o) => o,
            Err(_) => {
                return;
            }
        };

        let mut inner_value = match value.into_inner() {
            Ok(o) => o,
            Err(_) => {
                return;
            }
        };

        let error = unsafe {
            (self.destructor)(
                inner_value.as_mut_ptr() as *const c_void
            )
        };
        if error < 0 {
            println!("error dropping wasi_console_out_t: {error}");
        }
    }
}

impl fmt::Debug for wasi_console_out_t {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "wasi_console_out_t")
    }
}

impl io::Read for wasi_console_out_t {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let self_read = self.read;
        let mut data = self.get_data_mut("read")?;
        let result = unsafe {
            (self_read)(
                data.as_mut_ptr() as *const c_void,
                buf.as_mut_ptr() as *mut c_char,
                buf.len(),
            )
        };
        if result >= 0 {
            Ok(result as usize)
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("could not read from wasi_console_out_t: {result}"),
            ))
        }
    }
}

impl io::Write for wasi_console_out_t {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let self_write = self.write;
        let mut data = self.get_data_mut("write")?;
        let result = unsafe {
            (self_write)(
                data.as_mut_ptr() as *const c_void,
                buf.as_ptr() as *const c_char,
                buf.len(),
                false,
            )
        };
        if result >= 0 {
            Ok(result.try_into().unwrap_or(0))
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "could not write {} bytes to wasi_console_out_t: {result}",
                    buf.len()
                ),
            ))
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        let self_write = self.write;
        let mut data = self.get_data_mut("flush")?;
        let bytes_to_write = &[];
        let result: i64 = unsafe {
            (self_write)(
                data.as_mut_ptr() as *const c_void,
                bytes_to_write.as_ptr(),
                0,
                true,
            )
        };
        if result >= 0 {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("could not flush wasi_console_out_t: {result}"),
            ))
        }
    }
}

impl io::Seek for wasi_console_out_t {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let self_seek = self.seek;
        let mut data = self.get_data_mut("seek")?;
        let (id, pos) = match pos {
            SeekFrom::Start(s) => (0, s as i64),
            SeekFrom::End(s) => (1, s),
            SeekFrom::Current(s) => (2, s),
        };
        let result = unsafe {
            (self_seek)(
                data.as_mut_ptr() as *const c_void,
                id,
                pos,
            )
        };
        if result >= 0 {
            Ok(result.try_into().unwrap_or(0))
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("could not seek to {pos:?} wasi_console_out_t: {result}"),
            ))
        }
    }
}

impl VirtualFile for wasi_console_out_t {
    fn last_accessed(&self) -> u64 {
        0
    }
    fn last_modified(&self) -> u64 {
        0
    }
    fn created_time(&self) -> u64 {
        0
    }
    fn size(&self) -> u64 {
        self.get_data_mut("size")
            .map(|s| s.len() as u64)
            .unwrap_or(0)
    }
    fn set_len(&mut self, _: u64) -> Result<(), FsError> {
        Ok(())
    }
    fn unlink(&mut self) -> Result<(), FsError> {
        Ok(())
    }
}

/// Creates a new callback object that is being
#[no_mangle]
pub unsafe extern "C" fn wasi_console_out_new(
    read: WasiConsoleIoReadCallback,
    write: WasiConsoleIoWriteCallback,
    seek: WasiConsoleIoSeekCallback,
    destructor: WasiConsoleIoEnvDestructor,
    env_data: *const c_void,
    env_data_len: usize,
) -> *mut wasi_console_out_t {
    let data_vec: Vec<c_char> = std::slice::from_raw_parts(env_data as *const c_char, env_data_len).to_vec();

    Box::leak(Box::new(wasi_console_out_t {
        read,
        write,
        seek,
        destructor,
        data: Some(Box::new(Arc::new(Mutex::new(data_vec)))),
    }))
}

/// Creates a `wasi_console_out_t` callback object that does nothing
/// and redirects stdout / stderr to /dev/null
#[no_mangle]
pub unsafe extern "C" fn wasi_console_out_new_null() -> *mut wasi_console_out_t {
    let mut data = Vec::new();
    wasi_console_out_new(
        wasi_console_out_read_null,
        wasi_console_out_write_null,
        wasi_console_out_seek_null,
        wasi_console_out_delete_null,
        data.as_mut_ptr(),
        data.len(),
    )
}

extern "C" fn wasi_console_out_read_null(
    _: *const c_void,
    _: *mut c_char,
    _: usize,
) -> i64 {
    0
}
extern "C" fn wasi_console_out_write_null(
    _: *const c_void,
    _: *const c_char,
    _: usize,
    _: bool,
) -> i64 {
    0
}
extern "C" fn wasi_console_out_seek_null(
    _: *const c_void,
    _: c_char,
    _: i64,
) -> i64 {
    0
}

extern "C" fn wasi_console_out_delete_null(_: *const c_void) -> i64 {
    0
}

unsafe extern "C" fn wasi_console_out_read_memory(
    ptr: *const c_void,    /* = *Pipe */
    byte_ptr: *mut c_char, /* &[u8] bytes to read */
    max_bytes: usize,      /* max bytes to read */
) -> i64 {
    use std::io::Read;
    let ptr = ptr as *mut Pipe;
    let ptr = &mut *ptr;
    let slice = std::slice::from_raw_parts_mut(byte_ptr as *mut u8, max_bytes);
    match ptr.read(slice) {
        Ok(o) => o as i64,
        Err(_) => -1,
    }
}

unsafe extern "C" fn wasi_console_out_write_memory(
    ptr: *const c_void, /* = *Pipe */
    byte_ptr: *const c_char,
    byte_len: usize,
    flush: bool,
) -> i64 {
    use std::io::Write;
    
    let ptr = ptr as *mut Pipe;
    let ptr = &mut *ptr;

    if flush {
        match ptr.flush() {
            Ok(()) => 0,
            Err(_) => -1,
        }
    } else {
        let slice = std::slice::from_raw_parts(byte_ptr as *const u8, byte_len);
        match ptr.write(slice) {
            Ok(o) => o as i64,
            Err(_) => -1,
        }
    }
}

unsafe extern "C" fn wasi_console_out_seek_memory(
    ptr: *const c_void, /* = *Pipe */
    direction: c_char,
    seek_to: i64,
) -> i64 {
    use std::io::Seek;

    let ptr = ptr as *mut Pipe;
    let ptr = &mut *ptr;

    let seek_from = match direction {
        0 => std::io::SeekFrom::Start(seek_to.max(0) as u64),
        1 => std::io::SeekFrom::End(seek_to),
        2 => std::io::SeekFrom::Current(seek_to),
        _ => { return -1; },
    };

    match ptr.seek(seek_from) {
        Ok(o) => o as i64,
        Err(_) => -1,
    }
}

unsafe extern "C" fn wasi_console_out_delete_memory(
    ptr: *const c_void, /* = *Pipe */
) -> i64 {
    let ptr = ptr as *const Pipe;
    let _: Pipe = std::mem::transmute_copy(&*ptr); // dropped here, destructors run here
    0
}

/// Creates a new `wasi_console_out_t` which uses a memory buffer
/// for backing stdin / stdout / stderr
#[no_mangle]
pub unsafe extern "C" fn wasi_console_out_new_memory() -> *mut wasi_console_out_t {
    
    use std::mem::ManuallyDrop;

    let data = Pipe::new();
    let mut data = ManuallyDrop::new(data);
    let ptr: &mut Pipe = &mut data;

    wasi_console_out_new(
        wasi_console_out_read_memory,
        wasi_console_out_write_memory,
        wasi_console_out_seek_memory,
        wasi_console_out_delete_memory,
        ptr as *mut _ as *mut c_void,
        std::mem::size_of::<Pipe>(),
    )
}

#[no_mangle]
pub unsafe extern "C" fn wasi_console_out_delete(ptr: *mut wasi_console_out_t) -> bool {
    let _ = Box::from_raw(ptr);
    true
}

#[no_mangle]
pub unsafe extern "C" fn wasi_console_out_write_bytes(
    ptr: *mut wasi_console_out_t,
    buf: *const c_char,
    len: usize,
) -> i64 {
    use std::io::Write;
    let buf = buf as *const u8;
    let ptr = &mut *ptr;
    let read_slice = std::slice::from_raw_parts(buf, len);
    match ptr.write(read_slice) {
        Ok(o) => o as i64,
        Err(_) => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasi_console_out_write_str(
    ptr: *const wasi_console_out_t,
    buf: *const c_char,
) -> i64 {
    use std::io::Write;
    let c_str = std::ffi::CStr::from_ptr(buf);
    let as_bytes_with_nul = c_str.to_bytes();
    let ptr = &mut *(ptr as *mut wasi_console_out_t);
    match ptr.write(as_bytes_with_nul) {
        Ok(o) => o as i64,
        Err(_) => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasi_console_out_flush(ptr: *mut wasi_console_out_t) -> i64 {
    use std::io::Write;
    let ptr = &mut *ptr;
    match ptr.flush() {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasi_console_out_read_bytes(
    ptr: *const wasi_console_out_t,
    buf: *const c_char,
    read: usize,
) -> i64 {
    use std::io::Read;
    let ptr = &mut *(ptr as *mut wasi_console_out_t);
    let buf = buf as *mut u8;
    let slice = std::slice::from_raw_parts_mut(buf, read);
    match ptr.read(slice) {
        Ok(o) => o as i64,
        Err(_) => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasi_console_out_delete_str(buf: *mut c_char) {
    use std::ffi::CString;
    let _ = CString::from_raw(buf);
}

#[no_mangle]
pub unsafe extern "C" fn wasi_console_out_read_str(
    ptr: *const wasi_console_out_t,
    buf: *mut *mut c_char,
) -> i64 {
    use std::ffi::CString;
    use std::io::Read;

    const BLOCK_SIZE: usize = 1024;

    let mut target = Vec::new();
    let ptr = &mut *(ptr as *mut wasi_console_out_t);

    loop {
        let mut v = vec![0; BLOCK_SIZE];
        // read n bytes, maximum of 1024
        match ptr.read(&mut v) {
            Ok(0) => {
                break;
            }
            Ok(n) => {
                target.extend_from_slice(&v[..n]);
            }
            Err(_) => {
                return -1;
            }
        }
    }

    target.push(0);
    let len = target.len();
    let c_string = match CString::from_vec_with_nul(target.clone()) {
        Ok(o) => o,
        Err(_) => {
            return -1;
        }
    };

    *buf = CString::into_raw(c_string);
    len as i64
}

#[no_mangle]
pub unsafe extern "C" fn wasi_console_out_seek(
    ptr: *mut wasi_console_out_t,
    // 0 = from start
    // 1 = from end
    // 2 = from current position
    seek_dir: c_char,
    seek: i64,
) -> i64 {
    use std::io::Seek;

    let seek_pos = match seek_dir {
        0 => SeekFrom::Start(seek as u64),
        1 => SeekFrom::End(seek),
        2 => SeekFrom::Current(seek),
        _ => {
            return -1;
        }
    };

    let ptr = &mut *ptr;

    ptr.seek(seek_pos)
        .ok()
        .and_then(|p| p.try_into().ok())
        .unwrap_or(-1)
}

#[no_mangle]
pub unsafe extern "C" fn wasi_console_out_clone(
    ptr: *const wasi_console_out_t,
) -> *mut wasi_console_out_t {
    Box::leak(Box::new((&*ptr).clone()))
}

#[derive(Debug)]
#[allow(non_camel_case_types)]
pub struct wasi_config_t {
    inherit_stdout: Option<Box<wasi_console_out_t>>,
    inherit_stderr: Option<Box<wasi_console_out_t>>,
    inherit_stdin: Option<Box<wasi_console_out_t>>,
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
    config.inherit_stdout = Some(unsafe { Box::from_raw(wasi_console_out_new_null()) });
}

#[no_mangle]
pub extern "C" fn wasi_config_inherit_stdout(config: &mut wasi_config_t) {
    config.inherit_stdout = None;
}

#[no_mangle]
pub extern "C" fn wasi_config_capture_stderr(config: &mut wasi_config_t) {
    config.inherit_stderr = Some(unsafe { Box::from_raw(wasi_console_out_new_null()) });
}

#[no_mangle]
pub extern "C" fn wasi_config_inherit_stderr(config: &mut wasi_config_t) {
    config.inherit_stderr = None;
}

#[no_mangle]
pub extern "C" fn wasi_config_capture_stdin(config: &mut wasi_config_t) {
    config.inherit_stdin = Some(unsafe { Box::from_raw(wasi_console_out_new_null()) });
}

#[no_mangle]
pub extern "C" fn wasi_config_inherit_stdin(config: &mut wasi_config_t) {
    config.inherit_stdin = None;
}

#[allow(clippy::box_collection, clippy::redundant_allocation)]
#[repr(C)]
pub struct wasi_console_stdin_t {
    on_stdin: WasiConsoleIoOnStdinCallback,
    destructor: WasiConsoleIoEnvDestructor,
    data: Option<Box<Vec<c_char>>>,
}

impl wasi_console_stdin_t {
    fn get_data_mut(&mut self, op_id: &'static str) -> io::Result<&mut Vec<c_char>> {
        self.data.as_deref_mut().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("could not get env data ({op_id}) on wasi_console_stdin_t"),
            )
        })
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasi_console_stdin_new(
    on_stdin: WasiConsoleIoOnStdinCallback,
    destructor: WasiConsoleIoEnvDestructor,
    ptr: *const c_void,
    len: usize,
) -> *mut wasi_console_stdin_t {
    let data = std::slice::from_raw_parts(ptr as *const c_char, len);
    Box::leak(Box::new(wasi_console_stdin_t {
        on_stdin,
        destructor,
        data: Some(Box::new(data.to_vec())),
    }))
}

impl Drop for wasi_console_stdin_t {
    fn drop(&mut self) {
        let mut data = match self.data.take() {
            Some(s) => s,
            None => {
                return;
            }
        };

        let error = unsafe {
            (self.destructor)((*data).as_mut_ptr() as *const c_void)
        };

        if error < 0 {
            println!("error dropping wasi_console_stdin_t: {error}");
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasi_console_stdin_delete(ptr: *mut wasi_console_stdin_t) -> bool {
    let _ = Box::from_raw(ptr);
    true
}

#[allow(clippy::box_collection, clippy::redundant_allocation)]
#[derive(Clone)]
#[repr(C)]
pub struct wasi_console_stdin_response_t {
    // data that is written to the response
    response_data: Box<Vec<c_char>>,
}

#[no_mangle]
pub unsafe extern "C" fn wasi_console_stdin_response_write_bytes(
    response_t: &mut wasi_console_stdin_response_t,
    bytes: *const c_char,
    len: usize,
) -> i64 {
    let slice = std::slice::from_raw_parts(bytes, len);
    response_t.response_data.extend_from_slice(slice);
    slice.len() as i64
}

#[no_mangle]
pub unsafe extern "C" fn wasi_console_stdin_response_write_str(
    response_t: &mut wasi_console_stdin_response_t,
    str: *const c_char,
) -> i64 {
    let c_str = CStr::from_ptr(str);
    let bytes = c_str.to_bytes();
    wasi_console_stdin_response_write_bytes(
        response_t,
        bytes.as_ptr() as *const c_char,
        bytes.len(),
    )
}

impl fmt::Debug for wasi_console_stdin_t {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "wasi_console_stdin_t")
    }
}

impl io::Read for wasi_console_stdin_t {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut target = wasi_console_stdin_response_t {
            response_data: Box::new(Vec::new()),
        };
        let self_on_stdin = self.on_stdin;
        let data = self.get_data_mut("read")?;
        let result = unsafe {
            (self_on_stdin)(
                data.as_ptr() as *const c_void,
                buf.len(),
                &mut target,
            )
        };
        if result >= 0 {
            for (source, target) in target.response_data.iter().zip(buf.iter_mut()) {
                *target = *source as u8;
            }
            Ok(result as usize)
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("could not read from wasi_console_out_t: {result}"),
            ))
        }
    }
}

impl io::Write for wasi_console_stdin_t {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl io::Seek for wasi_console_stdin_t {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}

impl VirtualFile for wasi_console_stdin_t {
    fn last_accessed(&self) -> u64 {
        0
    }
    fn last_modified(&self) -> u64 {
        0
    }
    fn created_time(&self) -> u64 {
        0
    }
    fn size(&self) -> u64 {
        0
    }
    fn set_len(&mut self, _: u64) -> Result<(), FsError> {
        Ok(())
    }
    fn unlink(&mut self) -> Result<(), FsError> {
        Ok(())
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasi_config_overwrite_stdin(
    config: &mut wasi_config_t,
    stdin: *mut wasi_console_stdin_t,
) {
    config.state_builder.stdin(Box::from_raw(stdin));
}

#[no_mangle]
pub unsafe extern "C" fn wasi_config_overwrite_stdout(
    config: &mut wasi_config_t,
    stdout: *mut wasi_console_out_t,
) {
    config.state_builder.stdout(Box::from_raw(stdout));
}

#[no_mangle]
pub unsafe extern "C" fn wasi_config_overwrite_stderr(
    config: &mut wasi_config_t,
    stderr: *mut wasi_console_out_t,
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
            #include "string.h"
            #include "stdio.h"

            typedef struct {
                int invocation;
            } CustomWasiStdin;

            int64_t CustomWasiStdin_destructor(
                const void* env
            ) {
                (void)env;
                return 0;
            }

            int64_t CustomWasiStdin_onStdIn(
                const void* env,
                uintptr_t maxwrite,
                wasi_console_stdin_response_t* in
            ) {
                CustomWasiStdin* ptr = (CustomWasiStdin*)env;
                (void)maxwrite;
                if (ptr->invocation == 0) {
                    wasi_console_stdin_response_write_str(in, "hello");
                    ptr->invocation += 1;
                    return 5; // sizeof("hello")
                } else {
                    return 0;
                }
            }

            int main() {

                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);
                wasi_config_t* config = wasi_config_new("example_program");
                wasi_console_out_t* override_stdout = wasi_console_out_new_memory();
                wasi_console_out_t* override_stderr = wasi_console_out_new_memory();

                CustomWasiStdin stdin = { .invocation = 0 };
                wasi_console_stdin_t* override_stdin = wasi_console_stdin_new(
                    CustomWasiStdin_onStdIn,
                    CustomWasiStdin_destructor,
                    &stdin,
                    sizeof(stdin)
                );

                // Cloning the `wasi_console_out_t` does not deep-clone the
                // internal stream, since that is locked behind an Arc<Mutex<T>>.
                wasi_console_out_t* stdout_receiver = wasi_console_out_clone(override_stdout);
                wasi_console_out_t* stderr_receiver = wasi_console_out_clone(override_stderr);

                // The override_stdin ownership is moved to the config
                wasi_config_overwrite_stdin(config, override_stdin);
                wasi_config_overwrite_stdout(config, override_stdout);
                wasi_config_overwrite_stderr(config, override_stderr);

                // Load binary.
                FILE* file = fopen("tests/wasm-c-api/example/stdio.wasm", "rb");
                if (!file) {
                    printf("> Error loading module!\n");
                    return 1;
                }

                fseek(file, 0L, SEEK_END);
                size_t file_size = ftell(file);
                fseek(file, 0L, SEEK_SET);

                wasm_byte_vec_t binary;
                wasm_byte_vec_new_uninitialized(&binary, file_size);

                if (fread(binary.data, file_size, 1, file) != 1) {
                    printf("> Error loading module!\n");
                    return 1;
                }

                fclose(file);

                wasm_module_t* module = wasm_module_new(store, &binary);
                if (!module) {
                    printf("> Error compiling module!\n");
                    return 1;
                }

                // The env now has ownership of the config (using the custom stdout / stdin channels)
                wasi_env_t *wasi_env = wasi_env_new(store, config);
                if (!wasi_env) {
                    printf("> Error building WASI env!\n");
                    return 1;
                }

                wasm_importtype_vec_t import_types;
                wasm_module_imports(module, &import_types);

                wasm_extern_vec_t imports;
                wasm_extern_vec_new_uninitialized(&imports, import_types.size);
                wasm_importtype_vec_delete(&import_types);

                bool get_imports_result = wasi_get_imports(store, wasi_env, module, &imports);

                if (!get_imports_result) {
                    printf("Error getting WASI imports!\n");
                    return 1;
                }

                // The program should wait for a stdin, then print "stdout: $1" to stdout
                // and "stderr: $1" to stderr and exit.

                // Instantiate the module
                wasm_instance_t *instance = wasm_instance_new(store, module, &imports, NULL);
                if (!instance) {
                    printf("> Error instantiating module!\n");
                    return -1;
                }

                // Read the exports.
                wasm_extern_vec_t exports;
                wasm_instance_exports(instance, &exports);
                wasm_memory_t* mem = NULL;
                for (size_t i = 0; i < exports.size; i++) {
                    mem = wasm_extern_as_memory(exports.data[i]);
                    if (mem) {
                        break;
                    }
                }

                if (!mem) {
                    printf("Failed to create instance: Could not find memory in exports\n");
                    return -1;
                }
                wasi_env_set_memory(wasi_env, mem);

                // Get the _start function
                wasm_func_t* run_func = wasi_get_start_function(instance);
                if (run_func == NULL) {
                    printf("> Error accessing export!\n");
                    return 1;
                }

                // Run the _start function
                // Running the program should trigger the stdin to write "hello" to the stdin
                wasm_val_vec_t args = WASM_EMPTY_VEC;
                wasm_val_vec_t res = WASM_EMPTY_VEC;
                if (wasm_func_call(run_func, &args, &res)) {
                    printf("> Error calling function!\n");
                    return 1;
                }

                // Verify that the stdout / stderr worked as expected
                char* out;
                wasi_console_out_read_str(stdout_receiver, &out);
                assert(strcmp(out, "stdout: hello") == 0);
                wasi_console_out_delete_str(out);

                char* out2;
                wasi_console_out_read_str(stdout_receiver, &out2);
                assert(strcmp(out2, "") == 0);
                wasi_console_out_delete_str(out2);

                char* out3;
                wasi_console_out_read_str(stderr_receiver, &out3);
                assert(strcmp(out3, "stderr: hello") == 0);
                wasi_console_out_delete_str(out3);

                char* out4;
                wasi_console_out_read_str(stderr_receiver, &out4);
                assert(strcmp(out4, "") == 0);
                wasi_console_out_delete_str(out4);

                wasi_console_out_delete(stdout_receiver);
                wasi_console_out_delete(stderr_receiver);

                wasm_byte_vec_delete(&binary);
                wasm_module_delete(module);
                wasm_func_delete(run_func);
                wasi_env_delete(wasi_env);
                wasm_store_delete(store);
                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }
}
