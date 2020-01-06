use wasmer_runtime_core::{
    debug,
    memory::ptr::{Array, WasmPtr},
    memory::Memory,
    types::ValueType,
    vm::Ctx,
};

use std::io::Write;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GoArg {
    a: u32,
    b: u32,
    return_value: u64,
    array_len: u64,
}

unsafe impl ValueType for GoArg {}

pub fn debug(_ctx: &mut Ctx, _param1: i32) {
    debug!("go-js::debug {}", _param1);
    unimplemented!("debug")
}

pub fn runtime_wasm_exit(_ctx: &mut Ctx, _param1: i32) {
    debug!("go-js::runtime_wasm_exit {}", _param1);
    unimplemented!("runtime_wasm_exit")
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GoWasmWriteArg {
    _padding1: u64,
    fd: u64,
    ptr: u64,
    n: u32,
}

unsafe impl ValueType for GoWasmWriteArg {}

//use std::os::unix::io::FromRawFd;

pub fn runtime_wasm_write(ctx: &mut Ctx, arg: WasmPtr<GoWasmWriteArg>) {
    debug!("go-js::runtime_wasm_write");
    let memory = ctx.memory(0);
    let go_ww_arg = arg.deref(memory).unwrap().get();

    use std::fs::File;

    debug!("Found raw fd: {}", go_ww_arg.fd);
    //let mut file = unsafe { File::from_raw_fd(go_ww_arg.fd as _) };
    let data: WasmPtr<u8, Array> = WasmPtr::new(go_ww_arg.ptr as u32);
    let raw_buffer = unsafe {
        std::mem::transmute::<&[std::cell::Cell<u8>], &[u8]>(
            data.deref(memory, 0, go_ww_arg.n).unwrap(),
        )
    };
    debug!("Writing {} bytes to file", go_ww_arg.n);

    match go_ww_arg.fd {
        0 => panic!("Cannot write to stdin"),
        // stdout
        1 => {
            std::io::stdout()
                .lock()
                .write(raw_buffer)
                .expect("write to stdout");
        }
        // stderr
        2 => {
            std::io::stderr()
                .lock()
                .write(raw_buffer)
                .expect("write to stderr");
        }
        _ => unimplemented!("Writing to an fd that's not stdout or stderr!"),
    }
}

pub fn runtime_nano_time(ctx: &mut Ctx, arg: WasmPtr<GoArg>) {
    debug!("go-js::runtime_nano_time");
    let memory = ctx.memory(0);

    let now = std::time::SystemTime::now();
    let duration = now.duration_since(std::time::UNIX_EPOCH).unwrap();
    let go_arg = unsafe { arg.deref_mut(memory).unwrap().get_mut() };
    go_arg.return_value = duration.as_nanos() as u64;
    debug!("=> duration = {}", go_arg.return_value);
}

pub fn runtime_wall_time(_ctx: &mut Ctx, _param1: i32) {
    debug!("go-js::runtime_wall_time {}", _param1);
    unimplemented!("runtime_wall_time")
}

pub fn runtime_schedule_timeout_event(_ctx: &mut Ctx, _param1: i32) {
    debug!("go-js::runtime_wall_time {}", _param1);
    unimplemented!("runtime_wall_time")
}

pub fn runtime_clear_timeout_event(_ctx: &mut Ctx, _param1: i32) {
    debug!("go-js::runtime_schedule_timeout_event {}", _param1);
    unimplemented!("runtime_schedule_timeout_event");
}
pub fn runtime_get_random_data(ctx: &mut Ctx, arg: WasmPtr<GoArg>) {
    debug!("go-js::runtime_get_random_data");
    let memory = ctx.memory(0);
    let go_arg = arg.deref(memory).unwrap().get();
    let go_slice: WasmPtr<u8, Array> = WasmPtr::new(go_arg.return_value as u32);
    let go_slice_len = go_arg.array_len;

    debug!(
        "Writing {} random bytes at 0x{:X}",
        go_slice_len,
        go_slice.offset()
    );

    let mutable_slice = unsafe {
        std::mem::transmute::<&mut [std::cell::Cell<u8>], &mut [u8]>(
            go_slice.deref_mut(memory, 0, go_slice_len as u32).unwrap(),
        )
    };

    getrandom::getrandom(mutable_slice).expect("fill buffer with random bytes");
}

pub fn syscall_js_string_val(_ctx: &mut Ctx, _param1: i32) {
    debug!("go-js::syscall_js_string_val {}", _param1);
    unimplemented!("syscall_js_string_val");
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GoStrArg {
    _padding1: u64,
    value: u64,
    string_ptr: u64,
    string_len: u64,
    return_value: u64,
}

unsafe impl ValueType for GoStrArg {}

pub fn syscall_js_value_get(ctx: &mut Ctx, arg: WasmPtr<GoStrArg>) {
    debug!("go-js::syscall_js_value_get");
    let memory = ctx.memory(0);
    let go_str_arg = arg.deref(memory).unwrap().get();
    let wasm_str_ptr: WasmPtr<u8, Array> = WasmPtr::new(go_str_arg.string_ptr as u32);
    let wasm_str: &str = wasm_str_ptr
        .get_utf8_string(memory, go_str_arg.string_len as u32)
        .expect("string from go");

    let value = crate::Value::load_value(go_str_arg.value);

    debug!("Getting \"{}\" on {:?}", wasm_str, value);
    debug!("Rest of get not implemented, doing nothing!");
}

pub fn syscall_js_value_set(_ctx: &mut Ctx, _param1: i32) {
    debug!("go-js::syscall_js_value_set {}", _param1);
    unimplemented!("syscall_js_value_set");
}

pub fn syscall_js_value_index(_ctx: &mut Ctx, _param1: i32) {
    debug!("go-js::syscall_js_value_index {}", _param1);
    unimplemented!("syscall_js_value_index");
}

pub fn syscall_js_value_set_index(_ctx: &mut Ctx, _param1: i32) {
    debug!("go-js::syscall_js_value_set_index {}", _param1);
    unimplemented!("syscall_js_value_set_index");
}

pub fn syscall_js_value_call(_ctx: &mut Ctx, _param1: i32) {
    debug!("go-js::syscall_js_value_call {}", _param1);
    unimplemented!("syscall_js_value_call");
}

pub fn syscall_js_value_new(_ctx: &mut Ctx, _param1: i32) {
    debug!("go-js::syscall_js_value_new {}", _param1);
    unimplemented!("syscall_js_value_new");
}

pub fn syscall_js_value_length(_ctx: &mut Ctx, _param1: i32) {
    debug!("go-js::syscall_js_value_length {}", _param1);
    unimplemented!("syscall_js_value_length");
}

pub fn syscall_js_value_prepare_string(_ctx: &mut Ctx, _param1: i32) {
    debug!("go-js::syscall_js_value_prepare_string {}", _param1);
    unimplemented!("syscall_js_value_prepare_string");
}

pub fn syscall_js_value_load_string(_ctx: &mut Ctx, _param1: i32) {
    debug!("go-js::syscall_js_value_load_string {}", _param1);
    unimplemented!("syscall_js_value_load_string");
}

pub fn syscall_js_copy_bytes_to_js(_ctx: &mut Ctx, _param1: i32) {
    debug!("go-js::syscall_js_copy_bytes_to_js {}", _param1);
    unimplemented!("syscall_js_copy_bytes_to_js");
}
