mod syscalls;

use slab::Slab;
use std::ffi::c_void;
use std::collections::*;
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Nan,
    Null,
    True,
    False,
    Global,
    This,
    Number(u32),
    String(String),
    Bytes(Vec<u8>),
    Object {
        name: &'static str,
        values: HashMap<String, usize>
    },
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
            Value::Number(0) => out = (nan_head << 32) | 1,
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
            Value::Number(0),
            Value::Null,
            Value::True,
            Value::False,
            Value::Global,
            Value::This,
        ];

        static_array[arg as u32 as usize].clone()
    }
}

pub struct GoJsData<'a> {
    /// all the data values
    heap: Slab<Value>,
    getsp: Func<'a, (), (i32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoJsError {
    NotAnObject(usize),
}

impl<'a> GoJsData<'a> {
    pub(crate) const NULL: usize = 2;
    pub(crate) const TRUE: usize = 3;
    pub(crate) const FALSE: usize = 4;
    pub(crate) const GLOBAL: usize = 5;
    pub(crate) const MEM: usize = 5;
    pub(crate) const THIS: usize = 5;
    
    pub fn new(instance: &'a mut Instance) -> Result<Self, GoJsError> {
        let getsp = instance.func("getsp").expect("exported fn \"getsp\"");
        let mut go_js = GoJsData {
            heap: Slab::new(),
            getsp,
        };

        // initialize with expected default values
        go_js.heap_add(Value::Nan);
        go_js.heap_add(Value::Number(0));
        let null = go_js.heap_add(Value::Null);
        assert_eq!(null, Self::NULL);
        let _true = go_js.heap_add(Value::True);
        assert_eq!(_true, Self::TRUE);
        let _false = go_js.heap_add(Value::False);
        assert_eq!(_false, Self::FALSE);
        let global = go_js.heap_add(Value::Object {
            name: "global",
            values: HashMap::new(),
        });
        assert_eq!(global, Self::GLOBAL);
        let mem = go_js.heap_add(Value::Object {
            name: "mem",
            values: HashMap::new(),
        });
        assert_eq!(mem, Self::MEM);
        let this = go_js.heap_add(Value::Object {
            name: "this",
            values: HashMap::new(),
        });
        assert_eq!(this, Self::THIS);


        go_js.add_to_object(mem, "buffer")?;

        let fs = go_js.add_to_object(global, "fs")?;
        go_js.add_to_object(fs, "write")?;
        go_js.add_to_object(fs, "open")?;
        go_js.add_to_object(fs, "stat")?;
        go_js.add_to_object(fs, "fstat")?;
        go_js.add_to_object(fs, "read")?;
        go_js.add_to_object(fs, "mkdir")?;
        go_js.add_to_object(fs, "fsync")?;
        go_js.add_to_object(fs, "isDirectory")?;

        let constants = go_js.add_to_object(fs, "constants")?;
        // TODO: disambiguate between direct numbers and pointers
        go_js.insert_into_object(constants, "O_WRONLY", 1)?;
        go_js.insert_into_object(constants, "O_RDWR", 2)?;
        go_js.insert_into_object(constants, "O_CREAT", 64)?;
        go_js.insert_into_object(constants, "O_TRUNC", 512)?;
        go_js.insert_into_object(constants, "O_APPEND", 1024)?;
        go_js.insert_into_object(constants, "O_EXCL", 128)?;

        let crypto = go_js.add_to_object(global, "crypto")?;
        go_js.add_to_object(crypto, "getRandomValues")?;

        go_js.insert_into_object(this, "_pendingEvent", null)?;
        go_js.add_to_object(this, "_makeFuncWrapper")?;

        go_js.add_to_object(global, "Object")?;
        go_js.add_to_object(global, "Array")?;
        go_js.add_to_object(global, "Uint8Array")?;
        go_js.add_to_object(global, "Int16Array")?;
        go_js.add_to_object(global, "Int32Array")?;
        go_js.add_to_object(global, "Int8Array")?;
        go_js.add_to_object(global, "Uint16Array")?;
        go_js.add_to_object(global, "Uint32Array")?;
        go_js.add_to_object(global, "Float32Array")?;
        go_js.add_to_object(global, "Float64Array")?;
        go_js.add_to_object(global, "net_listener")?;

        let process = go_js.add_to_object(global, "process")?;
        go_js.add_to_object(process, "cwd")?;
        go_js.add_to_object(process, "chdir")?;

        let date = go_js.add_to_object(global, "Date")?;
        // TODO: review if this should be somewhere else
        go_js.add_to_object(date, "getTimezoneOffset")?;


        // TODO: add errors

        Ok(go_js)
    }

    fn heap_add(&mut self, v: Value) -> usize {
        self.heap.insert(v)
    }

    fn heap_get(&self, idx: usize) -> Option<&Value> {
        self.heap.get(idx)
    }
    fn heap_get_mut(&mut self, idx: usize) -> Option<&mut Value> {
        self.heap.get_mut(idx)
    }

    fn add_to_object(&mut self, obj_id: usize, property_name: &'static str) -> Result<usize, GoJsError> {
        let new_id = self.heap_add(Value::Object {
            name: property_name,
            values: HashMap::new(),
        });

        self.insert_into_object(obj_id, property_name, new_id)?;

        Ok(new_id)
    }

    // TODO: return errors here
    fn insert_into_object(&mut self, obj_id: usize, field: &str, value_ptr: usize) -> Result<(), GoJsError> {
        if let Some(Value::Object { values, .. }) = self.heap_get_mut(obj_id) {
            values.insert(field.to_string(), value_ptr);
            Ok(())
        } else {
            Err(GoJsError::NotAnObject(obj_id))
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
