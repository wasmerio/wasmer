#[macro_use]
extern crate wasmer_runtime_core;
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
    panic!("debug not yet implemented");
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

fn runtimeGetRandomData(_ctx: &mut Ctx, val: i32) {
    panic!("runtimeGetRandomData not yet implemented");
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
            "syscall/js.stringVal" => func!(crate::syscallJsStringVal),
            "syscall/js.valueGet" => func!(crate::syscallJsValueGet),
            "syscall/js.valueSet" => func!(crate::syscallJsValueSet),
            "syscall/js.valueSetIndex" => func!(crate::syscallJsValueSetIndex),
            "syscall/js.valueCall" => func!(crate::syscallJsValueCall),
            "syscall/js.valueNew" => func!(crate::syscallJsValueNew),
            "syscall/js.valuePrepareString" => func!(crate::syscallJsValuePrepareString),
            "syscall/js.valueLoadString" => func!(crate::syscallJsValueLoadString),
        },
    }
}
