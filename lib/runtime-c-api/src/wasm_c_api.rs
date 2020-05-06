//! entrypoints for the standard C API

use std::convert::{TryFrom, TryInto};
use std::ffi::c_void;
use std::mem;
use std::ptr;
use std::slice;
use std::sync::Arc;

use wasmer::compiler::compile;
use wasmer::error::{CallError, RuntimeError};
use wasmer::import::{ImportObject, LikeNamespace, Namespace};
use wasmer::module::Module;
use wasmer::units;
use wasmer::wasm;

// TODO: remove delete from macro generation, need to do that manually

// TODO: investigate marking things like the C++ API does (does not work in return position)
/*#[repr(transparent)]
pub struct Own<T>(T);

impl<T> std::ops::Deref<T> for Own<T> {
type Target = T;
fn deref(&self) -> Self::Target {
self.0
    }
}*/

// TODO: figure out whose responsibilty it is to check nullptrs, etc

// TODO: figure out where return values are defined

// this can be a wasmer-specific type with wasmer-specific functions for manipulating it
#[repr(C)]
pub struct wasm_config_t {}

#[no_mangle]
pub extern "C" fn wasm_config_new() -> *mut wasm_config_t {
    todo!("wasm_config_new")
    //ptr::null_mut()
}

#[repr(C)]
pub struct wasm_engine_t {}

#[no_mangle]
pub extern "C" fn wasm_engine_new() -> *mut wasm_engine_t {
    let mut wasmer_heap_string = "WASMER ENGINE".to_string();
    wasmer_heap_string.shrink_to_fit();
    let boxed_string: Box<String> = Box::new(wasmer_heap_string);
    Box::into_raw(boxed_string) as *mut wasm_engine_t
}

#[no_mangle]
pub extern "C" fn wasm_engine_delete(wasm_engine_address: *mut wasm_engine_t) {
    if !wasm_engine_address.is_null() {
        // this should not leak memory:
        // we should double check it to make sure though
        let _boxed_str: Box<String> = unsafe { Box::from_raw(wasm_engine_address as *mut String) };
    }
}

#[no_mangle]
pub extern "C" fn wasm_engine_new_with_config(
    _config_ptr: *mut wasm_config_t,
) -> *mut wasm_engine_t {
    wasm_engine_new()
}

#[repr(C)]
pub struct wasm_instance_t {
    inner: Arc<wasm::Instance>,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_new(
    _store: *mut wasm_store_t,
    module: *const wasm_module_t,
    imports: *const *const wasm_extern_t,
    // own
    _traps: *mut *mut wasm_trap_t,
) -> *mut wasm_instance_t {
    let module = &(&*module).inner;
    let module_imports = module.imports();
    let module_import_count = module_imports.len();
    let imports = argument_import_iter(imports);
    let mut import_object = ImportObject::new();
    let mut imports_processed = 0;
    for (
        wasm::ImportDescriptor {
            namespace,
            name,
            ty,
        },
        import,
    ) in module_imports.into_iter().zip(imports)
    {
        imports_processed += 1;
        // TODO: review this code and consider doing it without internal mutation (build up external data Namespaces and then register them with the ImportObject)
        if import_object.with_namespace(&namespace, |_| ()).is_none() {
            import_object.register(&namespace, Namespace::new());
        }
        match (ty, import.export.clone()) {
            (
                wasm::ExternDescriptor::Function(expected_signature),
                wasm::Export::Function { signature, .. },
            ) => {
                if expected_signature != *signature {
                    // TODO: report error
                    return ptr::null_mut();
                }

                import_object
                    .with_namespace_mut(
                        &namespace,
                        |ns: &mut (dyn LikeNamespace + Send)| -> Option<()> {
                            ns.maybe_insert(&name, import.export.clone())
                        },
                    )
                    .expect("failed to modify namespace: TODO handle this error");
            }
            (wasm::ExternDescriptor::Global(global_desc), wasm::Export::Global(export_global)) => {
                if global_desc != export_global.descriptor() {
                    // TODO: report error
                    return ptr::null_mut();
                }

                import_object
                    .with_namespace_mut(
                        &namespace,
                        |ns: &mut (dyn LikeNamespace + Send)| -> Option<()> {
                            ns.maybe_insert(&name, import.export.clone())
                        },
                    )
                    .expect("failed to modify namespace");
            }
            (wasm::ExternDescriptor::Memory(_), wasm::Export::Memory(_)) => todo!("memory"),
            (wasm::ExternDescriptor::Table(_), wasm::Export::Table(_)) => todo!("table"),
            _ => {
                // type mismatch: report error here
                return ptr::null_mut();
            }
        }
    }
    if module_import_count != imports_processed {
        // handle this error
        return ptr::null_mut();
    }

    let instance = Arc::new(
        module
            .instantiate(&import_object)
            .expect("failed to instantiate: TODO handle this error"),
    );
    Box::into_raw(Box::new(wasm_instance_t { inner: instance }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_delete(instance: *mut wasm_instance_t) {
    if !instance.is_null() {
        let _ = Box::from_raw(instance);
    }
}

struct CArrayIter<T: Sized + 'static> {
    cur_entry: *const *const T,
}

impl<T: Sized + 'static> CArrayIter<T> {
    fn new(array: *const *const T) -> Option<Self> {
        if array.is_null() {
            None
        } else {
            Some(CArrayIter { cur_entry: array })
        }
    }
}

impl<T: Sized + 'static> Iterator for CArrayIter<T> {
    type Item = &'static T;

    fn next(&mut self) -> Option<Self::Item> {
        let next_entry_candidate = unsafe { *self.cur_entry };
        if next_entry_candidate.is_null() {
            None
        } else {
            self.cur_entry = unsafe { self.cur_entry.add(1) };
            Some(unsafe { &*next_entry_candidate })
        }
    }
}

// reads from null-terminated array of `wasm_extern_t`s
unsafe fn argument_import_iter(
    imports: *const *const wasm_extern_t,
) -> Box<dyn Iterator<Item = &'static wasm_extern_t>> {
    CArrayIter::new(imports)
        .map(|it| Box::new(it) as _)
        .unwrap_or_else(|| Box::new(std::iter::empty()) as _)
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_exports(
    instance: *const wasm_instance_t,
    // TODO: review types on wasm_declare_vec, handle the optional pointer part properly
    out: *mut wasm_extern_vec_t,
) {
    let instance = &(&*instance).inner;
    // TODO: review name, does `into_iter` imply taking ownership?
    let mut extern_vec = instance
        .exports
        .into_iter()
        .map(|(name, export)| {
            let dynfunc = if let wasm::Export::Function { .. } = export {
                instance.exports.get(&name).ok()

            /*let sig_idx = SigRegistry::lookup_sig_index(signature);
            let trampoline = instance.module.runnable_module.get_trampoline(&instance.module.info, sig_idx).expect("wasm trampoline");
            Some(trampoline)*/
            } else {
                None
            };
            Box::into_raw(Box::new(wasm_extern_t {
                instance: Some(Arc::clone(instance)),
                dynfunc,
                export,
            }))
        })
        .collect::<Vec<*mut wasm_extern_t>>();
    extern_vec.shrink_to_fit();

    (*out).size = extern_vec.len();
    (*out).data = extern_vec.as_mut_ptr();
    // TODO: double check that the destructor will work correctly here
    mem::forget(extern_vec);
}

#[repr(C)]
pub struct wasm_module_t {
    inner: Arc<Module>,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_new(
    _store: *mut wasm_store_t,
    bytes: *const wasm_byte_vec_t,
) -> *mut wasm_module_t {
    let bytes = &*bytes;
    // TODO: review lifetime of byte slice
    let wasm_byte_slice: &[u8] = slice::from_raw_parts_mut(bytes.data, bytes.size);
    let result = compile(wasm_byte_slice);
    let module = match result {
        Ok(module) => module,
        Err(_) => {
            // TODO: error handling here
            return ptr::null_mut();
        }
    };

    Box::into_raw(Box::new(wasm_module_t {
        inner: Arc::new(module),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_delete(module: *mut wasm_module_t) {
    if !module.is_null() {
        let _ = Box::from_raw(module);
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_deserialize(
    _store: *mut wasm_store_t,
    bytes: *const wasm_byte_vec_t,
) -> *mut wasm_module_t {
    // TODO: read config from store and use that to decide which compiler to use

    let byte_slice = if bytes.is_null() || (&*bytes).into_slice().is_none() {
        // TODO: error handling here
        return ptr::null_mut();
    } else {
        (&*bytes).into_slice().unwrap()
    };
    let artifact = if let Ok(artifact) = wasmer::cache::Artifact::deserialize(byte_slice) {
        artifact
    } else {
        // TODO: error handling here
        return ptr::null_mut();
    };
    let compiler = wasmer::compiler::default_compiler();
    let module = if let Ok(module) = wasmer::module::load_from_cache(artifact, &compiler) {
        module
    } else {
        // TODO: error handling here
        return ptr::null_mut();
    };

    Box::into_raw(Box::new(wasm_module_t {
        inner: Arc::new(module),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_serialize(
    module_ptr: *const wasm_module_t,
    out_ptr: *mut wasm_byte_vec_t,
) {
    let module = &*module_ptr;
    let artifact = match module.inner.cache() {
        Ok(artifact) => artifact,
        Err(_) => return,
    };
    let mut byte_vec = match artifact.serialize() {
        Ok(mut byte_vec) => {
            byte_vec.shrink_to_fit();
            byte_vec
        }
        Err(_) => return,
    };
    // ensure we won't leak memory
    // TODO: use `Vec::into_raw_parts` when it becomes stable
    debug_assert_eq!(byte_vec.capacity(), byte_vec.len());
    (*out_ptr).size = byte_vec.len();
    (*out_ptr).data = byte_vec.as_mut_ptr();
    mem::forget(byte_vec);
}

#[repr(C)]
pub struct wasm_store_t {}

#[no_mangle]
pub extern "C" fn wasm_store_new(_wasm_engine: *mut wasm_engine_t) -> *mut wasm_store_t {
    let mut wasmer_heap_string = "WASMER STORE".to_string();
    wasmer_heap_string.shrink_to_fit();
    let boxed_string: Box<String> = Box::new(wasmer_heap_string);
    Box::into_raw(boxed_string) as *mut wasm_store_t
}

#[no_mangle]
pub extern "C" fn wasm_store_delete(wasm_store_address: *mut wasm_store_t) {
    if !wasm_store_address.is_null() {
        // this should not leak memory:
        // we should double check it to make sure though
        let _boxed_str: Box<String> = unsafe { Box::from_raw(wasm_store_address as *mut String) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_as_extern(func_ptr: *mut wasm_func_t) -> *mut wasm_extern_t {
    let func = &*func_ptr;
    match func.callback {
        CallbackType::WithEnv { .. } => todo!("wasm_func_as_extern for funcs with envs"),
        CallbackType::WithoutEnv(callback) => {
            let export = wasm::Export::Function {
                func: wasm::FuncPointer::new(callback as *const _),
                // TODO: figure out how to use `wasm::Context` correctly here
                ctx: wasm::Context::Internal,
                signature: Arc::clone(&func.functype),
            };

            Box::into_raw(Box::new(wasm_extern_t {
                instance: func.instance.clone(),
                dynfunc: None,
                export,
            }))
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_as_extern(
    global_ptr: *mut wasm_global_t,
) -> *mut wasm_extern_t {
    let global = &*global_ptr;
    Box::into_raw(Box::new(wasm_extern_t {
        // update this if global does hold onto an `instance`
        instance: None,
        dynfunc: None,
        export: wasm::Export::Global(global.global.clone()),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_as_extern(
    memory_ptr: *mut wasm_memory_t,
) -> *mut wasm_extern_t {
    let memory = &*memory_ptr;
    Box::into_raw(Box::new(wasm_extern_t {
        // update this if global does hold onto an `instance`
        instance: None,
        dynfunc: None,
        export: wasm::Export::Memory(memory.memory.clone()),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_as_func(extrn: *mut wasm_extern_t) -> *mut wasm_func_t {
    let extrn = &*extrn;
    match &extrn.export {
        wasm::Export::Function {
            ctx,
            ref signature,
            func,
        } => {
            let func = Box::new(wasm_func_t {
                instance: extrn.instance.clone(),
                dynfunc: extrn.dynfunc.as_ref().map(|r| r as *const _),
                functype: Arc::clone(signature),
                callback: match ctx {
                    wasm::Context::Internal => CallbackType::WithoutEnv(
                        // TOOD: fix this transmute, this isn't safe because the struct isn't transparent
                        mem::transmute(func),
                    ),
                    // this is probably doubly wrong: understand `External` better
                    wasm::Context::External(_) => CallbackType::WithoutEnv(
                        // TOOD: fix this transmute, this isn't safe because the struct isn't transparent
                        mem::transmute(func),
                    ),
                    unhandled_context => todo!(
                        "Handle other types of wasm Context: {:?}",
                        unhandled_context
                    ),
                },
            });
            Box::into_raw(func)
        }
        _ => return ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_as_global(extrn: *mut wasm_extern_t) -> *mut wasm_global_t {
    let extrn = &*extrn;
    match &extrn.export {
        wasm::Export::Global(global) => Box::into_raw(Box::new(wasm_global_t {
            global: global.clone(),
        })),
        _ => ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_as_memory(extrn: *mut wasm_extern_t) -> *mut wasm_memory_t {
    let extrn = &*extrn;
    match &extrn.export {
        wasm::Export::Memory(memory) => Box::into_raw(Box::new(wasm_memory_t {
            memory: memory.clone(),
        })),
        _ => ptr::null_mut(),
    }
}

#[allow(non_camel_case_types)]
pub type wasm_mutability_t = u8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
#[repr(u8)]
enum wasm_mutability_enum {
    WASM_CONST = 0,
    WASM_VAR,
}

impl wasm_mutability_enum {
    fn is_mutable(self) -> bool {
        self == Self::WASM_VAR
    }
}

impl TryFrom<wasm_mutability_t> for wasm_mutability_enum {
    type Error = &'static str;

    fn try_from(item: wasm_mutability_t) -> Result<Self, Self::Error> {
        Ok(match item {
            0 => wasm_mutability_enum::WASM_CONST,
            1 => wasm_mutability_enum::WASM_VAR,
            _ => return Err("wasm_mutability_t value out of bounds"),
        })
    }
}

#[allow(non_camel_case_types)]
pub type wasm_valkind_t = u8;

impl From<wasm_valkind_enum> for wasm::Type {
    fn from(other: wasm_valkind_enum) -> Self {
        use wasm_valkind_enum::*;
        match other {
            WASM_I32 => wasm::Type::I32,
            WASM_I64 => wasm::Type::I64,
            WASM_F32 => wasm::Type::F32,
            WASM_F64 => wasm::Type::F64,
            WASM_ANYREF => todo!("WASM_ANYREF variant not yet implemented"),
            WASM_FUNCREF => todo!("WASM_FUNCREF variant not yet implemented"),
        }
    }
}

impl From<wasm_valtype_t> for wasm::Type {
    fn from(other: wasm_valtype_t) -> Self {
        other.valkind.into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
#[repr(u8)]
pub enum wasm_valkind_enum {
    WASM_I32,
    WASM_I64,
    WASM_F32,
    WASM_F64,
    WASM_ANYREF = 128,
    WASM_FUNCREF,
}

#[repr(C)]
pub union wasm_val_inner {
    int32_t: i32,
    int64_t: i64,
    float32_t: f32,
    float64_t: f64,
    wref: *mut wasm_ref_t,
}

#[repr(C)]
pub struct wasm_val_t {
    kind: wasm_valkind_t,
    of: wasm_val_inner,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_val_copy(out_ptr: *mut wasm_val_t, val: *const wasm_val_t) {
    let val = &*val;
    (*out_ptr).kind = val.kind;
    (*out_ptr).of =
        // TODO: handle this error
        match val.kind.try_into().unwrap() {
            wasm_valkind_enum::WASM_I32 => wasm_val_inner { int32_t: val.of.int32_t },
            wasm_valkind_enum::WASM_I64 => wasm_val_inner { int64_t: val.of.int64_t },
            wasm_valkind_enum::WASM_F32 => wasm_val_inner { float32_t: val.of.float32_t },
            wasm_valkind_enum::WASM_F64 => wasm_val_inner { float64_t: val.of.float64_t },
            wasm_valkind_enum::WASM_ANYREF => wasm_val_inner { wref: val.of.wref },
            wasm_valkind_enum::WASM_FUNCREF => wasm_val_inner { wref: val.of.wref },
        };
}

#[no_mangle]
pub unsafe extern "C" fn wasm_val_delete(ptr: *mut wasm_val_t) {
    if !ptr.is_null() {
        // TODO: figure out where wasm_val is allocated first...
        let _ = Box::from_raw(ptr);
    }
}

impl TryFrom<wasm_valkind_t> for wasm_valkind_enum {
    type Error = &'static str;

    fn try_from(item: wasm_valkind_t) -> Result<Self, Self::Error> {
        Ok(match item {
            0 => wasm_valkind_enum::WASM_I32,
            1 => wasm_valkind_enum::WASM_I64,
            2 => wasm_valkind_enum::WASM_F32,
            3 => wasm_valkind_enum::WASM_F64,
            128 => wasm_valkind_enum::WASM_ANYREF,
            129 => wasm_valkind_enum::WASM_FUNCREF,
            _ => return Err("valkind value out of bounds"),
        })
    }
}

impl TryFrom<wasm_val_t> for wasm::Value {
    type Error = &'static str;

    fn try_from(item: wasm_val_t) -> Result<Self, Self::Error> {
        (&item).try_into()
    }
}

impl TryFrom<&wasm_val_t> for wasm::Value {
    type Error = &'static str;

    fn try_from(item: &wasm_val_t) -> Result<Self, Self::Error> {
        Ok(match item.kind.try_into()? {
            wasm_valkind_enum::WASM_I32 => wasm::Value::I32(unsafe { item.of.int32_t }),
            wasm_valkind_enum::WASM_I64 => wasm::Value::I64(unsafe { item.of.int64_t }),
            wasm_valkind_enum::WASM_F32 => wasm::Value::F32(unsafe { item.of.float32_t }),
            wasm_valkind_enum::WASM_F64 => wasm::Value::F64(unsafe { item.of.float64_t }),
            wasm_valkind_enum::WASM_ANYREF => return Err("ANYREF not supported at this time"),
            wasm_valkind_enum::WASM_FUNCREF => return Err("FUNCREF not supported at this time"),
        })
    }
}

impl TryFrom<wasm::Value> for wasm_val_t {
    type Error = &'static str;

    fn try_from(item: wasm::Value) -> Result<Self, Self::Error> {
        Ok(match item {
            wasm::Value::I32(v) => wasm_val_t {
                of: wasm_val_inner { int32_t: v },
                kind: wasm_valkind_enum::WASM_I32 as _,
            },
            wasm::Value::I64(v) => wasm_val_t {
                of: wasm_val_inner { int64_t: v },
                kind: wasm_valkind_enum::WASM_I64 as _,
            },
            wasm::Value::F32(v) => wasm_val_t {
                of: wasm_val_inner { float32_t: v },
                kind: wasm_valkind_enum::WASM_F32 as _,
            },
            wasm::Value::F64(v) => wasm_val_t {
                of: wasm_val_inner { float64_t: v },
                kind: wasm_valkind_enum::WASM_F64 as _,
            },
            wasm::Value::V128(_) => {
                return Err("128bit SIMD types not yet supported in Wasm C API")
            }
        })
    }
}

#[allow(non_camel_case_types)]
pub type wasm_func_callback_t =
    unsafe extern "C" fn(args: *const wasm_val_t, results: *mut wasm_val_t) -> *mut wasm_trap_t;

#[allow(non_camel_case_types)]
pub type wasm_func_callback_with_env_t = unsafe extern "C" fn(
    c_void,
    args: *const wasm_val_t,
    results: *mut wasm_val_t,
) -> *mut wasm_trap_t;

#[allow(non_camel_case_types)]
pub type wasm_env_finalizer_t = unsafe extern "C" fn(c_void);

#[allow(dead_code)]
#[repr(C)]
enum CallbackType {
    WithoutEnv(wasm_func_callback_t),
    WithEnv {
        callback: wasm_func_callback_with_env_t,
        env: c_void,
        finalizer: wasm_env_finalizer_t,
    },
}

#[repr(C)]
pub struct wasm_func_t {
    // hack to make it just work for now
    dynfunc: Option<*const wasm::DynFunc<'static>>,
    // this is how we ensure the instance stays alive
    instance: Option<Arc<wasm::Instance>>,
    functype: Arc<wasm::FuncSig>,
    callback: CallbackType,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_new(
    _store: *mut wasm_store_t,
    ft: *const wasm_functype_t,
    callback: wasm_func_callback_t,
) -> *mut wasm_func_t {
    // TODO: handle null pointers?
    let new_ft = wasm_functype_copy(ft as *mut _);
    let func_sig = functype_to_real_type(new_ft);
    let wasm_func = Box::new(wasm_func_t {
        instance: None,
        dynfunc: None,
        functype: func_sig,
        callback: CallbackType::WithoutEnv(callback),
    });
    Box::into_raw(wasm_func)
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_new_with_env(
    _store: *mut wasm_store_t,
    ft: *const wasm_functype_t,
    callback: wasm_func_callback_with_env_t,
    env: c_void,
    finalizer: wasm_env_finalizer_t,
) -> *mut wasm_func_t {
    // TODO: handle null pointers?
    let new_ft = wasm_functype_copy(ft as *mut _);
    let func_sig = functype_to_real_type(new_ft);
    let wasm_func = Box::new(wasm_func_t {
        instance: None,
        dynfunc: None,
        functype: func_sig,
        callback: CallbackType::WithEnv {
            callback,
            env,
            finalizer,
        },
    });
    Box::into_raw(wasm_func)
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_delete(func: *mut wasm_func_t) {
    if !func.is_null() {
        let _ = Box::from_raw(func);
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_call(
    func: *const wasm_func_t,
    args: *const wasm_val_t,
    results: *mut wasm_val_t,
) -> *mut wasm_trap_t {
    let func = &*func;

    let wasm_traps;
    if let Some(dynfunc) = func.dynfunc {
        let dynfunc = &*dynfunc;
        let mut params = vec![];
        for (i, param) in func.functype.params().iter().enumerate() {
            let arg = &(*args.add(i));

            match param {
                wasm::Type::I32 => {
                    if arg.kind != wasm_valkind_enum::WASM_I32 as u8 {
                        // todo: error handling
                        panic!("type mismatch!");
                    }
                    params.push(wasm::Value::I32(arg.of.int32_t));
                }
                wasm::Type::I64 => {
                    if arg.kind != wasm_valkind_enum::WASM_I64 as u8 {
                        // todo: error handling
                        panic!("type mismatch!");
                    }
                    params.push(wasm::Value::I64(arg.of.int64_t));
                }
                wasm::Type::F32 => {
                    if arg.kind != wasm_valkind_enum::WASM_F32 as u8 {
                        // todo: error handling
                        panic!("type mismatch!");
                    }
                    params.push(wasm::Value::F32(arg.of.float32_t));
                }
                wasm::Type::F64 => {
                    if arg.kind != wasm_valkind_enum::WASM_F64 as u8 {
                        // todo: error handling
                        panic!("type mismatch!");
                    }
                    params.push(wasm::Value::F64(arg.of.float64_t));
                }
                wasm::Type::V128 => todo!("Handle v128 case in wasm_func_call"),
            }
        }

        match dynfunc.call(&params) {
            Ok(wasm_results) => {
                for (i, actual_result) in wasm_results.iter().enumerate() {
                    let result_loc = &mut (*results.add(i));
                    match *actual_result {
                        wasm::Value::I32(v) => {
                            result_loc.of.int32_t = v;
                            result_loc.kind = wasm_valkind_enum::WASM_I32 as u8;
                        }
                        wasm::Value::I64(v) => {
                            result_loc.of.int64_t = v;
                            result_loc.kind = wasm_valkind_enum::WASM_I64 as u8;
                        }
                        wasm::Value::F32(v) => {
                            result_loc.of.float32_t = v;
                            result_loc.kind = wasm_valkind_enum::WASM_F32 as u8;
                        }
                        wasm::Value::F64(v) => {
                            result_loc.of.float64_t = v;
                            result_loc.kind = wasm_valkind_enum::WASM_F64 as u8;
                        }
                        wasm::Value::V128(_) => todo!("Handle v128 case in wasm_func_call"),
                    }
                }
                wasm_traps = ptr::null_mut();
            }
            Err(CallError::Runtime(e)) => {
                wasm_traps = Box::into_raw(Box::new(e)) as _;
            }
            Err(_) => {
                // TODO: handle this
                panic!("resolve error!");
            }
        }
    } else {
        wasm_traps = match func.callback {
            CallbackType::WithoutEnv(fp) => fp(args, results),
            _ => unimplemented!("Host calls with `wasm_func_call`"),
        };
    }

    wasm_traps
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_param_arity(func: *const wasm_func_t) -> usize {
    let func = &*func;
    func.functype.params().len()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_result_arity(func: *const wasm_func_t) -> usize {
    let func = &*func;
    func.functype.returns().len()
}

#[repr(C)]
pub struct wasm_global_t {
    // maybe needs to hold onto instance
    global: wasm::Global,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_new(
    _store: *mut wasm_store_t,
    gt_ptr: *const wasm_globaltype_t,
    val_ptr: *const wasm_val_t,
) -> *mut wasm_global_t {
    let gt = &*(gt_ptr as *const wasm::GlobalDescriptor);
    let val = &*val_ptr;
    let wasm_val = if let Ok(wv) = val.try_into() {
        wv
    } else {
        return ptr::null_mut();
    };
    let global = if gt.mutable {
        wasm::Global::new_mutable(wasm_val)
    } else {
        wasm::Global::new(wasm_val)
    };

    Box::into_raw(Box::new(wasm_global_t { global }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_delete(global: *mut wasm_global_t) {
    if !global.is_null() {
        let _ = Box::from_raw(global);
    }
}

// TODO: figure out if these should be deep or shallow copies
#[no_mangle]
pub unsafe extern "C" fn wasm_global_copy(global_ptr: *const wasm_global_t) -> *mut wasm_global_t {
    let wasm_global = &*global_ptr;

    // do shallow copy

    Box::into_raw(Box::new(wasm_global_t {
        global: wasm_global.global.clone(),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_get(global_ptr: *const wasm_global_t, out: *mut wasm_val_t) {
    let wasm_global = &*global_ptr;
    let value = wasm_global.global.get();
    *out = value.try_into().unwrap();
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_set(
    global_ptr: *mut wasm_global_t,
    val_ptr: *const wasm_val_t,
) {
    let wasm_global = &mut *global_ptr;
    let val = &*val_ptr;
    let value = val.try_into().unwrap();
    wasm_global.global.set(value);
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_same(
    global_ptr1: *const wasm_global_t,
    global_ptr2: *const wasm_global_t,
) -> bool {
    let wasm_global1 = &*global_ptr1;
    let wasm_global2 = &*global_ptr2;

    wasm_global1.global.same(&wasm_global2.global)
}

#[repr(C)]
pub struct wasm_memory_t {
    // maybe needs to hold onto instance
    memory: wasm::Memory,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_new(
    _store: *mut wasm_store_t,
    mt_ptr: *const wasm_memorytype_t,
) -> *mut wasm_memory_t {
    let md = (&*(mt_ptr as *const wasm::MemoryDescriptor)).clone();

    let memory = wasm::Memory::new(md).unwrap();
    Box::into_raw(Box::new(wasm_memory_t { memory }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_delete(memory: *mut wasm_memory_t) {
    if !memory.is_null() {
        let _ = Box::from_raw(memory);
    }
}

// TODO: figure out if these should be deep or shallow copies
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_copy(memory_ptr: *const wasm_memory_t) -> *mut wasm_memory_t {
    let wasm_memory = &*memory_ptr;

    // do shallow copy

    Box::into_raw(Box::new(wasm_memory_t {
        memory: wasm_memory.memory.clone(),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_type(
    _memory_ptr: *const wasm_memory_t,
) -> *mut wasm_memorytype_t {
    todo!("wasm_memory_type")
}

// get a raw pointer into bytes
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_data(memory_ptr: *mut wasm_memory_t) -> *mut u8 {
    let memory = &mut *memory_ptr;
    mem::transmute::<&[std::cell::Cell<u8>], &[u8]>(&memory.memory.view()[..]) as *const [u8]
        as *const u8 as *mut u8
}

// size in bytes
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_data_size(memory_ptr: *const wasm_memory_t) -> usize {
    let memory = &*memory_ptr;
    memory.memory.size().bytes().0
}

// size in pages
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_size(memory_ptr: *const wasm_memory_t) -> u32 {
    let memory = &*memory_ptr;
    memory.memory.size().0 as _
}

// delta is in pages
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_grow(memory_ptr: *mut wasm_memory_t, delta: u32) -> bool {
    let memory = &mut *memory_ptr;
    memory.memory.grow(units::Pages(delta)).is_ok()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_same(
    memory_ptr1: *const wasm_memory_t,
    memory_ptr2: *const wasm_memory_t,
) -> bool {
    let wasm_memory1 = &*memory_ptr1;
    let wasm_memory2 = &*memory_ptr2;

    wasm_memory1.memory.same(&wasm_memory2.memory)
}

macro_rules! wasm_declare_own {
    ($name:ident) => {
        paste::item! {
            #[repr(C)]
            pub struct [<wasm_ $name _t>] {}

            #[no_mangle]
            pub extern "C" fn [<wasm_ $name _delete>](_arg: *mut [<wasm_ $name _t>]) {
                todo!("in generated delete")
            }
        }
    };
}

macro_rules! wasm_declare_vec_inner {
    ($name:ident) => {
        paste::item! {


            #[no_mangle]
            pub unsafe extern "C" fn [<wasm_ $name _vec_new_uninitialized>](out: *mut [<wasm_ $name _vec_t>], length: usize) {
                // TODO: actually implement this
                [<wasm_ $name _vec_new>](out, length);
            }

            #[no_mangle]
            pub unsafe extern "C" fn [<wasm_ $name _vec_new_empty>](out: *mut [<wasm_ $name _vec_t>]) {
                // TODO: actually implement this
                [<wasm_ $name _vec_new>](out, 0);
            }

            #[no_mangle]
            pub unsafe extern "C" fn [<wasm_ $name _vec_delete>](ptr: *mut [<wasm_ $name _vec_t>]) {
                let vec = &mut *ptr;
                if !vec.data.is_null() {
                    Vec::from_raw_parts(vec.data, vec.size, vec.size);
                    vec.data = ptr::null_mut();
                    vec.size = 0;
                }
            }
        }
    }
}

macro_rules! wasm_declare_vec {
    ($name:ident) => {
        paste::item! {
            #[repr(C)]
            pub struct [<wasm_ $name _vec_t>] {
                pub size: usize,
                pub data: *mut [<wasm_ $name _t>],
            }

            impl [<wasm_ $name _vec_t>] {
                pub unsafe fn into_slice(&self) -> Option<&[[<wasm_ $name _t>]]>{
                    if self.data.is_null() {
                        return None;
                    }

                    Some(slice::from_raw_parts(self.data, self.size))
                }
            }

            #[no_mangle]
            pub unsafe extern "C" fn [<wasm_ $name _vec_new>](out: *mut [<wasm_ $name _vec_t>], length: usize, /* TODO: this arg count is wrong)*/) {
                let mut bytes: Vec<[<wasm_ $name _t>]> = Vec::with_capacity(length);
                let pointer = bytes.as_mut_ptr();
                debug_assert!(bytes.len() == bytes.capacity());
                (*out).data = pointer;
                (*out).size = length;
                mem::forget(bytes);
            }
        }
        wasm_declare_vec_inner!($name);
    };
}

macro_rules! wasm_declare_boxed_vec {
    ($name:ident) => {
        paste::item! {
            #[repr(C)]
            pub struct [<wasm_ $name _vec_t>] {
                pub size: usize,
                pub data: *mut *mut [<wasm_ $name _t>],
            }

            // TODO: do this properly
            impl [<wasm_ $name _vec_t>] {
                pub unsafe fn into_slice(&self) -> Option<&[*mut [<wasm_ $name _t>]]>{
                    if self.data.is_null() {
                        return None;
                    }

                    Some(slice::from_raw_parts(self.data, self.size))
                }
            }

            #[no_mangle]
            pub unsafe extern "C" fn [<wasm_ $name _vec_new>](out: *mut [<wasm_ $name _vec_t>], length: usize, /* TODO: this arg count is wrong)*/) {
                let mut bytes: Vec<*mut [<wasm_ $name _t>]> = Vec::with_capacity(length);
                let pointer = bytes.as_mut_ptr();
                debug_assert!(bytes.len() == bytes.capacity());
                (*out).data = pointer;
                (*out).size = length;
                mem::forget(bytes);
            }
        }
        wasm_declare_vec_inner!($name);
    };
}

macro_rules! wasm_declare_ref_base {
    ($name:ident) => {
        wasm_declare_own!($name);
        paste::item! {
            #[no_mangle]
            pub extern "C" fn [<wasm_ $name _copy>](_arg: *const [<wasm_ $name _t>]) -> *mut [<wasm_ $name _t>] {
                todo!("in generated declare ref base");
                //ptr::null_mut()
            }

            // TODO: finish this...

        }
    };
}

#[allow(non_camel_case_types)]
pub type wasm_byte_t = u8;
wasm_declare_vec!(byte);

wasm_declare_ref_base!(ref);

// opaque type which is a `RuntimeError`
#[repr(C)]
pub struct wasm_trap_t {}

#[no_mangle]
pub unsafe extern "C" fn wasm_trap_delete(trap: *mut wasm_trap_t) {
    if !trap.is_null() {
        let _ = Box::from_raw(trap as *mut RuntimeError);
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_trap_message(
    trap: *const wasm_trap_t,
    out_ptr: *mut wasm_byte_vec_t,
) {
    let re = &*(trap as *const RuntimeError);
    // this code assumes no nul bytes appear in the message
    let mut message = format!("{}\0", re);
    message.shrink_to_fit();

    // TODO use `String::into_raw_parts` when it gets stabilized
    (*out_ptr).size = message.as_bytes().len();
    (*out_ptr).data = message.as_mut_ptr();
    mem::forget(message);
}

// in trap/RuntimeError we need to store
// 1. message
// 2. origin (frame); frame contains:
//    1. func index
//    2. func offset
//    3. module offset
//    4. which instance this was apart of

/*#[no_mangle]
pub unsafe extern "C" fn wasm_trap_trace(trap: *const wasm_trap_t, out_ptr: *mut wasm_frame_vec_t) {
    let re = &*(trap as *const RuntimeError);
    todo!()
}*/

#[repr(C)]
pub struct wasm_extern_t {
    // Hack for Wasm functions: TODO clean this up
    dynfunc: Option<wasm::DynFunc<'static>>,
    // this is how we ensure the instance stays alive
    instance: Option<Arc<wasm::Instance>>,
    export: wasm::Export,
}
wasm_declare_boxed_vec!(extern);

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct wasm_valtype_t {
    valkind: wasm_valkind_enum,
}

wasm_declare_vec!(valtype);

#[no_mangle]
pub extern "C" fn wasm_valtype_new(kind: wasm_valkind_t) -> *mut wasm_valtype_t {
    let kind_enum = if let Ok(kind_enum) = kind.try_into() {
        kind_enum
    } else {
        return ptr::null_mut();
    };
    let valtype = wasm_valtype_t { valkind: kind_enum };
    let valtype_ptr = Box::new(valtype);
    Box::into_raw(valtype_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn wasm_valtype_delete(valtype: *mut wasm_valtype_t) {
    if !valtype.is_null() {
        let _ = Box::from_raw(valtype);
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_valtype_kind(valtype: *const wasm_valtype_t) -> wasm_valkind_t {
    if valtype.is_null() {
        // TODO: handle error
        panic!("wasm_valtype_kind: argument is null pointer");
    }
    return (*valtype).valkind as wasm_valkind_t;
}

//wasm_declare_ref!(trap);
//wasm_declare_ref!(foreign);

// opaque type wrapping `wasm::GlobalDescriptor`
#[repr(C)]
pub struct wasm_globaltype_t {}

wasm_declare_vec!(globaltype);

#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_new(
    // own
    valtype: *mut wasm_valtype_t,
    mutability: wasm_mutability_t,
) -> *mut wasm_globaltype_t {
    wasm_globaltype_new_inner(valtype, mutability).unwrap_or(ptr::null_mut())
}

#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_delete(globaltype: *mut wasm_globaltype_t) {
    if !globaltype.is_null() {
        let _ = Box::from_raw(globaltype as *mut wasm::GlobalDescriptor);
    }
}

unsafe fn wasm_globaltype_new_inner(
    // own
    valtype_ptr: *mut wasm_valtype_t,
    mutability: wasm_mutability_t,
) -> Option<*mut wasm_globaltype_t> {
    let me: wasm_mutability_enum = mutability.try_into().ok()?;
    let valtype = *valtype_ptr;
    let gd = Box::new(wasm::GlobalDescriptor {
        mutable: me.is_mutable(),
        ty: valtype.into(),
    });
    wasm_valtype_delete(valtype_ptr);

    Some(Box::into_raw(gd) as *mut wasm_globaltype_t)
}

// opaque type wrapping `wasm::MemoryDescriptor`
#[repr(C)]
pub struct wasm_memorytype_t {}

wasm_declare_vec!(memorytype);

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct wasm_limits_t {
    min: u32,
    max: u32,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memorytype_new(
    limits: *const wasm_limits_t,
) -> *mut wasm_memorytype_t {
    let limits = *limits;
    let min_pages = units::Pages(limits.min as _);
    // TODO: investigate if `0` is in fact a sentinel value here
    let max_pages = if limits.max == 0 {
        None
    } else {
        Some(units::Pages(limits.max as _))
    };
    Box::into_raw(Box::new(
        wasm::MemoryDescriptor::new(min_pages, max_pages, false)
            .expect("creating a new memory descriptor"),
    )) as *mut wasm_memorytype_t
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memorytype_delete(memorytype: *mut wasm_memorytype_t) {
    if !memorytype.is_null() {
        let _ = Box::from_raw(memorytype as *mut wasm::MemoryDescriptor);
    }
}

// TODO: fix memory leak
// this function leaks memory because the returned limits pointer is not owned
#[no_mangle]
pub unsafe extern "C" fn wasm_memorytype_limits(
    mt: *const wasm_memorytype_t,
) -> *const wasm_limits_t {
    let md = &*(mt as *const wasm::MemoryDescriptor);
    Box::into_raw(Box::new(wasm_limits_t {
        min: md.minimum.bytes().0 as _,
        max: md.maximum.map(|max| max.bytes().0 as _).unwrap_or(0),
    }))
}

// opaque type wrapping `Arc<wasm::FuncSig>`
#[repr(C)]
pub struct wasm_functype_t {}

wasm_declare_vec!(functype);

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_new(
    // own
    params: *mut wasm_valtype_vec_t,
    // own
    results: *mut wasm_valtype_vec_t,
) -> *mut wasm_functype_t {
    wasm_functype_new_inner(params, results).unwrap_or(ptr::null_mut())
}

unsafe fn wasm_functype_new_inner(
    // own
    params: *mut wasm_valtype_vec_t,
    // own
    results: *mut wasm_valtype_vec_t,
) -> Option<*mut wasm_functype_t> {
    let params = &*params;
    let results = &*results;
    let params: Vec<wasm::Type> = params
        .into_slice()?
        .iter()
        .copied()
        .map(Into::into)
        .collect::<Vec<_>>();
    let results: Vec<wasm::Type> = results
        .into_slice()?
        .iter()
        .copied()
        .map(Into::into)
        .collect::<Vec<_>>();

    let funcsig = Arc::new(wasm::FuncSig::new(params, results));
    Some(Arc::into_raw(funcsig) as *mut wasm_functype_t)
}

#[no_mangle]
pub extern "C" fn wasm_functype_delete(arg: *mut wasm_functype_t) {
    if !arg.is_null() {
        let _ = unsafe { Arc::from_raw(arg as *mut wasm::FuncSig) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_copy(arg: *mut wasm_functype_t) -> *mut wasm_functype_t {
    if !arg.is_null() {
        let funcsig = functype_to_real_type(arg);
        let new_funcsig = Arc::clone(&funcsig);
        // don't free the original Arc
        mem::forget(funcsig);
        Arc::into_raw(new_funcsig) as *mut wasm_functype_t
    } else {
        ptr::null_mut()
    }
}

unsafe fn functype_to_real_type(arg: *mut wasm_functype_t) -> Arc<wasm::FuncSig> {
    Arc::from_raw(arg as *mut wasm::FuncSig)
}

#[repr(C)]
pub struct wasm_frame_t {}

wasm_declare_vec!(frame);
