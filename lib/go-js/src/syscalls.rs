use crate::{cast_value, load_value_start, store_value, GoJsData, NumVal, Value};
use wasmer_runtime_core::{
    debug,
    memory::ptr::{Array, WasmPtr},
    memory::Memory,
    types::ValueType,
    vm::Ctx,
};

use std::io::Write;
use std::time;

fn get_go_js_data(ctx: &Ctx) -> &GoJsData {
    unsafe { &*(ctx.data as *const GoJsData) }
}

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

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct GoWallTimeArg {
    _padding1: u64,
    seconds: u64,
    subsec_nanos: u32,
}

unsafe impl ValueType for GoWallTimeArg {}

pub fn runtime_wall_time(ctx: &mut Ctx, arg: WasmPtr<GoWallTimeArg>) {
    debug!("go-js::runtime_wall_time");
    let memory = ctx.memory(0);
    let go_arg_cell = arg.deref(memory).unwrap();
    let now = time::SystemTime::now();
    let unix_ts = now.duration_since(time::UNIX_EPOCH).unwrap();

    go_arg_cell.set(GoWallTimeArg {
        _padding1: 0,
        seconds: unix_ts.as_secs(),
        subsec_nanos: unix_ts.subsec_nanos(),
    });
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
    let data = get_go_js_data(ctx);

    let go_str_arg = arg.deref(memory).unwrap().get();
    let wasm_str_ptr: WasmPtr<u8, Array> = WasmPtr::new(go_str_arg.string_ptr as u32);
    let wasm_str: &str = wasm_str_ptr
        .get_utf8_string(memory, go_str_arg.string_len as u32)
        .expect("string from go");
    let value = load_value_start(go_str_arg.value).inner();
    // TODO: cast here when casting is fixed

    debug!("Getting \"{}\" on {:?}", wasm_str, value);
    let result = data
        .reflect_get(value as usize, wasm_str)
        .expect("reflected value");
    debug!("Found {:?}", result);
    let setter: &mut GoStrArg = unsafe { arg.deref_mut(memory).unwrap().get_mut() };
    // TODO Review JS for this and maybe submit a patch to wasabi
    setter.return_value = store_value(result);
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

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct GoValueCallArg {
    _padding1: u64,                // 0
    value: u64,                    // 8
    field_str: WasmPtr<u8, Array>, // 16
    _padding2: u32,                // 20
    field_str_len: u32,            //24
    _padding3: u32,                // 28
    values: WasmPtr<u64, Array>,   // 32
    _padding4: u32,                // 36
    values_len: u32,               // 40
    _padding5: u32,                // 44
    _padding6: u64,                // 48
    result: u64,                   // 56
    success_bool: u64,             // 64
}

unsafe impl ValueType for GoValueCallArg {}

pub fn syscall_js_value_call(ctx: &mut Ctx, arg: WasmPtr<GoValueCallArg>) {
    debug!("go-js::syscall_js_value_call");
    let (memory, data) = unsafe { ctx.memory_and_data_mut::<GoJsData>(0) };
    let go_arg: GoValueCallArg = arg
        .deref(memory)
        .expect("arg to syscall_js_value_new in bounds")
        .get();

    let dereffed_value = load_value_start(go_arg.value).inner();
    // TOOD: review casting
    let value = dereffed_value; //cast_value(dereffed_value as u64);

    let field_str = go_arg
        .field_str
        .get_utf8_string(memory, go_arg.field_str_len)
        .expect("field_str in syscall_js_value_call");

    let slice = go_arg
        .values
        .deref(memory, 0, go_arg.values_len)
        .expect("slice in syscall_js_value_new");
    let mut args = vec![];
    for item in slice.iter() {
        let dereffed_val = load_value_start(item.get());
        args.push(dereffed_val);
    }

    // do logic that can fail here so we can easily capture it
    let mut inner = || {
        let m = data.reflect_get(value, field_str)?;
        data.reflect_apply(m.inner(), value, &args)
    };

    let setter: &mut GoValueCallArg = unsafe { arg.deref_mut(memory).unwrap().get_mut() };
    if let Some(result) = inner() {
        setter.result = result.inner() as u64;
        setter.success_bool = 1;
    } else {
        // TODO: store error
        setter.success_bool = 0;
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct GoValueNewArg {
    _padding1: u64,           // 0
    value: u64,               // 8
    ptr: WasmPtr<u64, Array>, // 16
    _padding2: u32,           // 20
    len: u32,                 // 24
    _padding3: u32,           // 28
    _something: u64,          // 32
    result: u64,              // 40
    _other_result: u64,       // 48
}

unsafe impl ValueType for GoValueNewArg {}

pub fn syscall_js_value_new(ctx: &mut Ctx, arg: WasmPtr<GoValueNewArg>) {
    debug!("go-js::syscall_js_value_new");
    let (memory, data) = unsafe { ctx.memory_and_data_mut::<GoJsData>(0) };

    let go_arg = arg
        .deref(memory)
        .expect("arg to syscall_js_value_new in bounds")
        .get();

    let dereffed_value = load_value_start(go_arg.value).inner();
    // TODO cast value when casting values works
    let value = dereffed_value; //cast_value(dereffed_value as u64);
    let slice = go_arg
        .ptr
        .deref(memory, 0, go_arg.len as u32)
        .expect("slice in syscall_js_value_new");
    let mut args = vec![];
    for item in slice.iter() {
        let dereffed_val = load_value_start(item.get());
        args.push(cast_value(dereffed_val.inner() as u64));
    }
    debug!("Found {} args: {:#?}", go_arg.len, &args);

    let result = data.reflect_construct(value as usize, &args).unwrap();
    let setter: &mut GoValueNewArg = unsafe { arg.deref_mut(memory).unwrap().get_mut() };
    setter.result = result.inner() as u64;
    setter._other_result = 1;
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

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct GoJsCopyBytesToJsArg {
    _padding1: u64,           // 0
    dest: u64,                // 8
    src: WasmPtr<u64, Array>, // 16
    _padding2: u32,           // 20
    len: u64,                 // 24
    _something: u64,          // 32
    result: u64,              // 40
    success_bool: u64,        // 48
}

unsafe impl ValueType for GoJsCopyBytesToJsArg {}

pub fn syscall_js_copy_bytes_to_js(ctx: &mut Ctx, arg: WasmPtr<GoJsCopyBytesToJsArg>) {
    debug!("go-js::syscall_js_copy_bytes_to_js");

    let (memory, data) = unsafe { ctx.memory_and_data_mut::<GoJsData>(0) };
    let go_arg = arg
        .deref(memory)
        .expect("arg to syscall_js_copy_bytes_to_js in bounds")
        .get();

    let slice = go_arg
        .src
        .deref(memory, 0, go_arg.len as u32)
        .expect("slice in syscall_js_value_new");

    let setter: &mut GoJsCopyBytesToJsArg = unsafe { arg.deref_mut(memory).unwrap().get_mut() };

    let amount_copied = match load_value_start(go_arg.dest) {
        // TODO: investigate this -- this seems to strongly imply ther's a bug somewhere but I don't
        // think it's in `load_value_start` and the value here is is clearly the correct value...
        // if this isn't a bug, then this ABI is really complicated and unstructured
        NumVal::Pointer(p) | NumVal::Value(p) => {
            // look it up
            match data.heap_get_mut(p) {
                Some(Value::Array(a)) => {
                    let mut idx = 0;
                    for item in slice.iter() {
                        let dereffed_val = load_value_start(item.get());
                        if let Some(entry) = a.get_mut(idx) {
                            *entry = dereffed_val;
                        } else {
                            break;
                        }
                        idx += 1;
                    }
                    idx
                }
                _ => {
                    setter.success_bool = 0;
                    return;
                }
            }
        } /*
          _ => {
              setter.success_bool = 0;
              return;
          }
          */
    };

    debug!("Wrote {} bytes", amount_copied);
    setter.result = amount_copied as u64;
    setter.success_bool = 1;
}
