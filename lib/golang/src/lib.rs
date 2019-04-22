#[macro_use]
extern crate wasmer_runtime_core;
use rand::Rng;
use std::ffi::c_void;
use std::mem::transmute;
use std::time::{SystemTime, UNIX_EPOCH};
use wasmer_runtime_core::{
    error::CallResult, import::ImportObject, module::Module, types::Value, vm::Ctx, Instance,
};

/// We check if a provided module is an Golang generated one
pub fn is_golang_module(module: &Module) -> bool {
    for (_, import_name) in &module.info().imported_functions {
        let namespace = module
            .info()
            .namespace_table
            .get(import_name.namespace_index);
        let field = module.info().name_table.get(import_name.name_index);
        if field == "debug" && namespace == "go" {
            return true;
        }
    }
    false
}

pub fn run_golang_instance(
    _module: &Module,
    instance: &mut Instance,
    path: &str,
    args: Vec<&str>,
) -> CallResult<()> {
    let main_func = instance.dyn_func("run")?;
    let num_params = main_func.signature().params().len();
    let _result = match num_params {
        2 => {
            //          TODO  let (argc, argv) = store_module_arguments(instance.context_mut(), path, args);
            instance.call("run", &[Value::I32(0), Value::I32(0)])?;
            //          TODO  instance.call("run", &[Value::I32(argc as i32), Value::I32(argv as i32)])?;
        }
        0 => {
            instance.call("run", &[])?;
        }
        _ => panic!(
            "The golang main function has received an incorrect number of params {}",
            num_params
        ),
    };

    Ok(())
}

fn debug(_ctx: &mut Ctx, val: i32) {
    println!("{}", val);
}

fn runtimeWasmExit(_ctx: &mut Ctx, val: i32) {
    panic!("runtimeWasmExit not yet implemented");
}

fn runtimeWasmWrite(_ctx: &mut Ctx, val: i32) {
    panic!("runtimeWasmWrite not yet implemented");
}

fn runtimeNanotime(ctx: &mut Ctx, val: i32) {
    let time_now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let time_nanos = time_now.as_secs() * 1_000_000_000 + time_now.subsec_nanos() as u64;
    setInt64(ctx, val + 8, time_nanos);
}

// Gets a little endian u64 from the memory at the given index
fn getInt64(ctx: &Ctx, ptr: i32) -> u64 {
    let mem = ctx.memory(0);
    let mut bytes: [u8; 8] = Default::default();
    use std::cell::Cell;
    let slice = mem.view::<u8>()[(ptr as usize)..((ptr + 8) as usize)].as_ptr() as *mut Cell<u8>
        as *const u8;
    let slice = unsafe { std::slice::from_raw_parts(slice, 8) };
    bytes.copy_from_slice(&slice[0..8]);
    u64::from_le_bytes(bytes)
}

// Sets a little endian u64 to the memory at the given index
fn setInt64(ctx: &mut Ctx, ptr: i32, val: u64) {
    let val_le_bytes = val.to_le_bytes();
    let mem = ctx.memory(0);
    for (mem_byte, val_byte) in mem.view::<u8>()[(ptr as usize)..]
        .iter()
        .zip(val_le_bytes.iter())
    {
        mem_byte.set(*val_byte);
    }
    //ctx.memory(0).view::<u64>()[ptr as usize].set(val);
}

fn runtimeWalltime(_ctx: &mut Ctx, val: i32) {
    panic!("runtimeWalltime not yet implemented");
}

fn runtimeScheduleCallback(_ctx: &mut Ctx, val: i32) {
    panic!("runtimeScheduleCallback not yet implemented");
}

fn runtimeClearScheduledCallback(_ctx: &mut Ctx, val: i32) {
    panic!("runtimeClearScheduledCallback not yet implemented");
}

/// Fills a slice of bytes with random values
fn runtimeGetRandomData(ctx: &mut Ctx, idx: i32) {
    let idx = idx + 8;
    let array = getInt64(ctx, idx);
    let len = getInt64(ctx, idx + 8);
    let mem = ctx.memory(0);
    // fill the u8 bytes with random values
    let mut rng = rand::thread_rng();
    for mem_byte in mem.view::<u8>()[(array as usize)..((array + len) as usize)].iter() {
        mem_byte.set(rng.gen());
    }
}

fn syscallJsStringVal(_ctx: &mut Ctx, val: i32) {
    panic!("syscallJsStringVal not yet implemented");
}

fn syscallJsValueGet(_ctx: &mut Ctx, val: i32) {
    panic!("syscallJsValueGet not yet implemented");
}

fn syscallJsValueSet(_ctx: &mut Ctx, val: i32) {
    panic!("syscallJsValueSet not yet implemented");
}

fn syscallJsValueSetIndex(_ctx: &mut Ctx, val: i32) {
    panic!("syscallJsValueSetIndex not yet implemented");
}

fn syscallJsValueCall(_ctx: &mut Ctx, val: i32) {
    panic!("syscallJsValueCall not yet implemented");
}

fn syscallJsValueNew(_ctx: &mut Ctx, val: i32) {
    panic!("syscallJsValueNew not yet implemented");
}

fn syscallJsValuePrepareString(_ctx: &mut Ctx, val: i32) {
    panic!("syscallJsValuePrepareString not yet implemented");
}

fn syscallJsValueLoadString(_ctx: &mut Ctx, val: i32) {
    panic!("syscallJsValueLoadString not yet implemented");
}

fn runtime_schedule_timeout_event(_ctx: &mut Ctx, val: i32) {
    panic!("runtime_schedule_timeout_event not yet implemented");
}

fn runtime_clear_timeout_event(_ctx: &mut Ctx, val: i32) {
    panic!("runtime_clear_timeout_event not yet implemented");
}

fn syscall_js_value_index(_ctx: &mut Ctx, val: i32) {
   panic!("syscall_js_value_index not yet implemented");
}

fn syscall_js_value_length(_ctx: &mut Ctx, val: i32){
   panic!("syscall_js_value_length not yet implemented");
}

pub fn generate_golang_env() -> ImportObject {
    imports! {
        "go" => {
            "debug" => func!(crate::debug),
            "runtime.wasmExit" => func!(crate::runtimeWasmExit),
            "runtime.wasmWrite" => func!(crate::runtimeWasmWrite),
            "runtime.nanotime" => func!(crate::runtimeNanotime),
            "runtime.walltime" => func!(crate::runtimeWalltime),
            "runtime.scheduleCallback" => func!(crate::runtimeScheduleCallback),
            "runtime.clearScheduledCallback" => func!(crate::runtimeClearScheduledCallback),
            "runtime.getRandomData" =>  func!(crate::runtimeGetRandomData),
            "runtime.clearTimeoutEvent" => func!(crate::runtime_clear_timeout_event),
            "runtime.scheduleTimeoutEvent" => func!(crate::runtime_schedule_timeout_event),
            "syscall/js.stringVal" => func!(crate::syscallJsStringVal),
            "syscall/js.valueGet" => func!(crate::syscallJsValueGet),
            "syscall/js.valueSet" => func!(crate::syscallJsValueSet),
            "syscall/js.valueSetIndex" => func!(crate::syscallJsValueSetIndex),
            "syscall/js.valueCall" => func!(crate::syscallJsValueCall),
            "syscall/js.valueIndex" => func!(crate::syscall_js_value_index),
            "syscall/js.valueLength" => func!(crate::syscall_js_value_length),
            "syscall/js.valueNew" => func!(crate::syscallJsValueNew),
            "syscall/js.valuePrepareString" => func!(crate::syscallJsValuePrepareString),
            "syscall/js.valueLoadString" => func!(crate::syscallJsValueLoadString),
        },
    }
}
