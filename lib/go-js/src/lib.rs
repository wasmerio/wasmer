mod syscalls;

use slab::Slab;
use std::collections::*;
use std::ffi::c_void;
use std::io::Write;
use wasmer_runtime_core::{
    debug, func, import::ImportObject, imports, module::Module, Func, Instance,
};

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
    Array(Vec<NumVal>),
    Object {
        name: &'static str,
        values: HashMap<String, NumVal>,
    },
    Memory {
        address: u64,
        len: u32,
    },
    Undefined,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NumVal {
    Pointer(usize),
    Value(usize),
}

impl NumVal {
    pub fn inner(&self) -> usize {
        match self {
            Self::Pointer(v) | Self::Value(v) => *v,
        }
    }
}

const NAN_HEAD: u64 = 0x7FF80000;
impl Value {
    pub(crate) fn into_bytes(&self) -> u64 {
        let out: u64;
        match self {
            Value::Nan => {
                out = NAN_HEAD << 32;
            }
            Value::Number(0) => out = (NAN_HEAD << 32) | 1,
            Value::Number(n) => {
                // TODO: this is a 64 bit float? but it's written as a 32bit?
                out = *n as u64;
            }
            Value::Undefined => out = 0,
            Value::Null => out = (NAN_HEAD << 32) | 2,
            Value::True => out = (NAN_HEAD << 32) | 3,
            Value::False => out = (NAN_HEAD << 32) | 4,
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

#[derive(Debug)]
pub(crate) enum NumType {
    Float(i64),
    Int(i64),
}

impl NumType {
    pub fn inner(&self) -> i64 {
        match self {
            Self::Float(n) | Self::Int(n) => *n,
        }
    }
}

// equivirent to the js load value but does not load from memory itselg
pub(crate) fn cast_value(num: u64) -> i64 {
    let float = f64::from_bits(num);
    if !float.is_nan() {
        float as i64
    } else {
        num as u32 as i64
    }
}

// the first half of the JS `load_value`; TODO: find a better name
pub(crate) fn load_value_start(v: u64) -> NumVal {
    if (v >> 32) == NAN_HEAD {
        NumVal::Pointer((v & (u32::max_value() as u64)) as usize)
    } else {
        NumVal::Value(v as usize)
    }
}

pub(crate) fn store_value(v: NumVal) -> u64 {
    match v {
        NumVal::Pointer(p) => ((NAN_HEAD as u64) << 32) | p as u64,
        NumVal::Value(v) => v as u64,
    }
}

pub struct GoJsData<'a> {
    /// all the data values
    heap: Slab<Value>,
    getsp: Func<'a, (), (i32)>,
}

impl<'a> std::fmt::Debug for GoJsData<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoJsData")
            .field("heap", &self.heap)
            .finish()
    }
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
    //pub(crate) const MEM: usize = 6;
    pub(crate) const THIS: usize = 6;

    pub fn new(instance: &'a mut Instance) -> Result<Self, GoJsError> {
        let getsp = instance.func("getsp").expect("exported fn \"getsp\"");
        let mut go_js = GoJsData {
            heap: Slab::new(),
            getsp,
        };

        // initialize with expected default values
        go_js.heap_add(Value::Nan);
        go_js.heap_add(Value::Number(0));
        let null = NumVal::Pointer(go_js.heap_add(Value::Null));
        assert_eq!(null, NumVal::Pointer(Self::NULL));
        let _true = go_js.heap_add(Value::True);
        assert_eq!(_true, Self::TRUE);
        let _false = go_js.heap_add(Value::False);
        assert_eq!(_false, Self::FALSE);
        let global = go_js.heap_add(Value::Object {
            name: "global",
            values: HashMap::new(),
        });
        assert_eq!(global, Self::GLOBAL);
        /*let mem = go_js.heap_add(Value::Object {
            name: "mem",
            values: HashMap::new(),
        });
        assert_eq!(mem, Self::MEM);*/
        let this = go_js.heap_add(Value::Object {
            name: "this",
            values: HashMap::new(),
        });
        assert_eq!(this, Self::THIS);

        //go_js.add_to_object(mem, "buffer")?;

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
        go_js.insert_into_object(constants, "O_WRONLY", NumVal::Value(1))?;
        go_js.insert_into_object(constants, "O_RDWR", NumVal::Value(2))?;
        go_js.insert_into_object(constants, "O_CREAT", NumVal::Value(64))?;
        go_js.insert_into_object(constants, "O_TRUNC", NumVal::Value(512))?;
        go_js.insert_into_object(constants, "O_APPEND", NumVal::Value(1024))?;
        go_js.insert_into_object(constants, "O_EXCL", NumVal::Value(128))?;

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
        let _enoent = go_js.add_io_error("ENOENT")?;
        let _eexist = go_js.add_io_error("EEXIST")?;

        Ok(go_js)
    }

    fn add_io_error(&mut self, name: &'static str) -> Result<usize, GoJsError> {
        let enoent = self.heap_add(Value::Object {
            name,
            values: HashMap::new(),
        });
        let code = self.heap_add(Value::String(name.to_owned()));
        self.insert_into_object(enoent, "code", NumVal::Pointer(code))?;
        Ok(enoent)
    }

    fn heap_add(&mut self, v: Value) -> usize {
        self.heap.insert(v)
    }

    pub(crate) fn heap_get(&self, idx: usize) -> Option<&Value> {
        self.heap.get(idx)
    }
    fn heap_get_mut(&mut self, idx: usize) -> Option<&mut Value> {
        self.heap.get_mut(idx)
    }

    fn add_to_object(
        &mut self,
        obj_id: usize,
        property_name: &'static str,
    ) -> Result<usize, GoJsError> {
        let new_id = self.heap_add(Value::Object {
            name: property_name,
            values: HashMap::new(),
        });

        self.insert_into_object(obj_id, property_name, NumVal::Pointer(new_id))?;

        Ok(new_id)
    }

    fn insert_into_object(
        &mut self,
        obj_id: usize,
        field: &str,
        num_val: NumVal,
    ) -> Result<(), GoJsError> {
        if let Some(Value::Object { values, .. }) = self.heap_get_mut(obj_id) {
            values.insert(field.to_string(), num_val);
            Ok(())
        } else {
            Err(GoJsError::NotAnObject(obj_id))
        }
    }

    pub(crate) fn reflect_get(&self, target: usize, property_key: &str) -> Option<NumVal> {
        debug!("getting {} on {}", property_key, target);
        if let Value::Object { values, .. } = self.heap_get(target)? {
            values.get(property_key).cloned()
        } else {
            None
        }
    }

    pub(crate) fn reflect_construct(&mut self, target: usize, args: &[i64]) -> Option<NumVal> {
        let name = if let Value::Object { name, .. } = self.heap_get(target)? {
            *name
        } else {
            return None;
        };
        debug!("Reflectively constructing {}", name);

        match name {
            "Uint8Array" => Some(NumVal::Pointer(
                self.heap_add(Value::Array(
                    std::iter::repeat(NumVal::Value(0))
                        .take(*args.get(0)? as usize)
                        .collect(),
                )),
            )),
            "Date" => Some(NumVal::Pointer(target)),
            // "net_listener"
            _ => None,
        }
    }

    pub(crate) fn get_object_name(&self, object: usize) -> Option<&str> {
        if let Value::Object { name, .. } = self.heap_get(object)? {
            Some(name)
        } else {
            None
        }
    }

    pub(crate) fn reflect_apply(
        &mut self,
        target: usize,
        object: usize,
        args: &[NumVal],
    ) -> Option<NumVal> {
        let target_name = self.get_object_name(target)?;
        let object_name = self.get_object_name(object)?;

        debug!("reflect_apply: {}.{}", object_name, target_name);
        Some(match (object_name, target_name) {
            ("Date", "getTimezoneOffset") => NumVal::Value(0),
            ("this", "_makeFuncWrapper") => {
                let wf = self.heap_add(Value::Object {
                    name: "wrappedFunc",
                    values: HashMap::new(),
                });
                // from wasabi, "maybe don't create an object here?"
                self.add_to_object(wf, "this").ok()?;
                self.insert_into_object(wf, "id", args.get(0).cloned()?)
                    .expect("insert into object in reflect_apply _makeFuncWrapper");
                NumVal::Pointer(wf)
            }
            ("fs", "write") => {
                if let Value::Array(a) = self.heap_get(args[1].inner())? {
                    let stdout = std::io::stdout();
                    let mut writer = stdout.lock();
                    for e in a.iter().take_while(|n| n.inner() != 0) {
                        writer.write(&e.inner().to_le_bytes()).unwrap();
                    }
                    args.get(3).cloned()?
                } else {
                    panic!("fs.write found something that's not an array: investigate to see if it makes sense or if it's a bug on the program's part")
                }
            }
            _ => return None,
        })
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
    let data: Box<GoJsData> = Box::new(GoJsData::new(instance).expect("construct go js data"));
    let data_ptr = Box::into_raw(data) as *mut c_void;

    instance.context_mut().data = data_ptr;

    let run: Func<(i32, i32), ()> = instance.func("run").expect("run fn exported");

    run.call(0, 0).expect("run")
}
