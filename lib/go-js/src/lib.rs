mod syscalls;

use std::ffi::c_void;
use wasmer_runtime_core::{func, import::ImportObject, imports, module::Module, Func, Instance};

/// Returns whether or not a [`Module`] is a go-js Module
// TODO: clean up this fn with the changes made to the WASI one
pub fn is_go_js_module(module: &Module) -> bool {
    if module.info().imported_functions.is_empty() {
        return false;
    }
    for (_, import_name) in &module.info().imported_functions {
        let namespace = module
            .info()
            .namespace_table
            .get(import_name.namespace_index);
        if namespace != "go" {
            return false;
        }
    }
    true
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Value {
    Nan,
    Zero,
    Null,
    True,
    False,
    Global,
    This,
    Number(u32),
    Undefined,
}

impl Value {
    pub(crate) fn into_bytes(&self) -> u64 {
        const nan_head: u64 = 0x7FF80000;
        let out: u64;
        match self {
            Value::Nan => {
                out = nan_head << 32;
            }
            Value::Zero => out = (nan_head << 32) | 1,
            Value::Number(n) => {
                // TODO: this is a 64 bit float? but it's written as a 32bit?
                out = *n as u64;
            }
            Value::Undefined => out = 0,
            Value::Null => out = (nan_head << 32) | 2,
            Value::True => out = (nan_head << 32) | 3,
            Value::False => out = (nan_head << 32) | 4,
            _ => unimplemented!("Unhandeled Value"),
        }

        out
    }

    pub(crate) fn load_value(arg: u64) -> Self {
        if arg == 0 {
            return Self::Undefined;
        }
        let dbl = unsafe { std::mem::transmute::<u64, f64>(arg) };
        if !dbl.is_nan() {
            return Self::Nan;
        }
        let static_array = [
            Value::Nan,
            Value::Zero,
            Value::Null,
            Value::True,
            Value::False,
            Value::Global,
            Value::This,
        ];

        static_array[arg as u32 as usize]
    }
}

pub struct GoJsData<'a> {
    values: [Value; 7],
    getsp: Func<'a, (), (i32)>,
}

impl<'a> GoJsData<'a> {
    pub fn new(instance: &'a mut Instance) -> Self {
        let getsp = instance.func("getsp").expect("exported fn \"getsp\"");
        GoJsData {
            values: [
                Value::Nan,
                Value::Zero,
                Value::Null,
                Value::True,
                Value::False,
                Value::Global,
                Value::This,
            ],
            getsp,
        }
    }
}

// TODO passing args

pub fn generate_import_object() -> ImportObject {
    imports! {
        "go" => {
            "debug" => func!(syscalls::debug),
            "runtime.wasmExit" => func!(syscalls::runtime_wasm_exit),
            "runtime.wasmWrite" => func!(syscalls::runtime_wasm_write),
            "runtime.nanotime" => func!(syscalls::runtime_nano_time),
            "runtime.walltime" => func!(syscalls::runtime_wall_time),
            "runtime.scheduleTimeoutEvent" => func!(syscalls::runtime_schedule_timeout_event),
            "runtime.clearTimeoutEvent" => func!(syscalls::runtime_clear_timeout_event),
            "runtime.getRandomData" => func!(syscalls::runtime_get_random_data),
            "syscall/js.stringVal" => func!(syscalls::syscall_js_string_val),
            "syscall/js.valueGet" => func!(syscalls::syscall_js_value_get),
            "syscall/js.valueSet" => func!(syscalls::syscall_js_value_set),
            "syscall/js.valueIndex" => func!(syscalls::syscall_js_value_index),
            "syscall/js.valueSetIndex" => func!(syscalls::syscall_js_value_set_index),
            "syscall/js.valueCall" => func!(syscalls::syscall_js_value_call),
            "syscall/js.valueNew" => func!(syscalls::syscall_js_value_new),
            "syscall/js.valueLength" => func!(syscalls::syscall_js_value_length),
            "syscall/js.valuePrepareString" => func!(syscalls::syscall_js_value_prepare_string),
            "syscall/js.valueLoadString" => func!(syscalls::syscall_js_value_load_string),
            "syscall/js.copyBytesToJS" => func!(syscalls::syscall_js_copy_bytes_to_js),
        },
    }
}

pub fn run_go_js_instance(instance: &mut Instance) {
    let data = Box::new(GoJsData::new(instance));
    let data_ptr = Box::into_raw(data) as *mut c_void;

    instance.context_mut().data = data_ptr;

    let run: Func<(i32, i32), ()> = instance.func("run").expect("run fn exported");

    run.call(0, 0).expect("run")
}
