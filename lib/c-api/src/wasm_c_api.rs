//! entrypoints for the standard C API

use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::ffi::c_void;
use std::mem;
use std::ptr::{self, NonNull};
use std::slice;
use std::sync::Arc;

use wasmer::{
    CompilerConfig, Engine, Exports, Extern, ExternType, Function, FunctionType, Global,
    GlobalType, ImportObject, Instance, JITEngine, Memory, MemoryType, Module, Mutability, Pages,
    RuntimeError, Store, Tunables, Val, ValType,
};

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
pub struct wasm_engine_t {
    inner: Arc<dyn Engine + Send + Sync>,
}

fn get_default_compiler_config() -> Box<dyn CompilerConfig> {
    // TODO: use cfg-if
    #[cfg(feature = "cranelift-backend")]
    Box::new(wasmer::CraneliftConfig::default())
    /*
    #[cfg(feature = "singlepass-backend")]
    Box::new(wasmer::SinglepassConfig::default())

    #[cfg(feature = "llvm-backend")]
    Box::new(wasmer::LLVMConfig::default())
        */
}

#[no_mangle]
pub extern "C" fn wasm_engine_new() -> NonNull<wasm_engine_t> {
    let compiler_config: Box<dyn CompilerConfig> = get_default_compiler_config();
    let tunables = Tunables::default();
    let engine: Arc<dyn Engine + Send + Sync> = Arc::new(JITEngine::new(compiler_config, tunables));
    let wasm_engine = Box::new(wasm_engine_t { inner: engine });
    unsafe { NonNull::new_unchecked(Box::into_raw(wasm_engine)).cast::<wasm_engine_t>() }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_engine_delete(wasm_engine_address: Option<NonNull<wasm_engine_t>>) {
    if let Some(e_inner) = wasm_engine_address {
        // this should probably be a no-op
        Box::from_raw(e_inner.as_ptr());
    }
}

#[no_mangle]
pub extern "C" fn wasm_engine_new_with_config(
    _config_ptr: *mut wasm_config_t,
) -> NonNull<wasm_engine_t> {
    wasm_engine_new()
}

#[repr(C)]
pub struct wasm_instance_t {
    inner: Arc<Instance>,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_new(
    store: Option<NonNull<wasm_store_t>>,
    module: *const wasm_module_t,
    imports: *const *const wasm_extern_t,
    // own
    _traps: *mut *mut wasm_trap_t,
) -> Option<NonNull<wasm_instance_t>> {
    let wasm_module = &(&*module).inner;
    let module_imports = wasm_module.imports();
    let module_import_count = module_imports.len();
    let imports = argument_import_iter(imports);
    let mut imports_processed = 0;
    let mut org_map: HashMap<String, Exports> = HashMap::new();
    for (import_type, import) in module_imports.into_iter().zip(imports) {
        imports_processed += 1;
        let entry = org_map
            .entry(import_type.module().to_string())
            .or_insert_with(Exports::new);

        match (import_type.ty(), &import.inner) {
            (ExternType::Function(expected_signature), Extern::Function(f)) => {
                if expected_signature != f.ty() {
                    // TODO: report error
                    return None;
                }
            }
            (ExternType::Global(global_ty), Extern::Global(extern_global)) => {
                if global_ty != extern_global.ty() {
                    // TODO: report error
                    return None;
                }
            }
            (ExternType::Memory(memory_ty), Extern::Memory(extern_memory)) => {
                if memory_ty != extern_memory.ty() {
                    // TODO: report error
                    return None;
                }
            }
            (ExternType::Table(table_ty), Extern::Table(extern_table)) => {
                if table_ty != extern_table.ty() {
                    // TODO: report error
                    return None;
                }
            }
            _ => {
                // type mismatch: report error here
                return None;
            }
        }
        entry.insert(import_type.name(), import.inner.clone())
    }
    if module_import_count != imports_processed {
        // handle this error
        return None;
    }

    let mut import_object = ImportObject::new();
    for (ns, exports) in org_map {
        import_object.register(ns, exports);
    }

    let instance = Arc::new(
        Instance::new(wasm_module, &import_object)
            .expect("failed to instantiate: TODO handle this error"),
    );
    Some(NonNull::new_unchecked(Box::into_raw(Box::new(
        wasm_instance_t { inner: instance },
    ))))
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
    let mut extern_vec = instance
        .exports
        .iter()
        .map(|(name, r#extern)| {
            let function = if let Extern::Function { .. } = r#extern {
                instance.exports.get_function(&name).ok().cloned()

            /*let sig_idx = SigRegistry::lookup_sig_index(signature);
            let trampoline = instance.module.runnable_module.get_trampoline(&instance.module.info, sig_idx).expect("wasm trampoline");
            Some(trampoline)*/
            } else {
                None
            };
            Box::into_raw(Box::new(wasm_extern_t {
                instance: Some(Arc::clone(instance)),
                inner: r#extern.clone(),
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
    store_ptr: Option<NonNull<wasm_store_t>>,
    bytes: *const wasm_byte_vec_t,
) -> Option<NonNull<wasm_module_t>> {
    let bytes = &*bytes;
    // TODO: review lifetime of byte slice
    let wasm_byte_slice: &[u8] = slice::from_raw_parts_mut(bytes.data, bytes.size);
    let store_ptr: NonNull<Store> = store_ptr?.cast::<Store>();
    let store = store_ptr.as_ref();
    let result = Module::from_binary(store, wasm_byte_slice);
    let module = match result {
        Ok(module) => module,
        Err(_) => {
            // TODO: error handling here
            return None;
        }
    };

    Some(NonNull::new_unchecked(Box::into_raw(Box::new(
        wasm_module_t {
            inner: Arc::new(module),
        },
    ))))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_delete(module: Option<NonNull<wasm_module_t>>) {
    if let Some(m_inner) = module {
        let _ = Box::from_raw(m_inner.as_ptr());
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_deserialize(
    store_ptr: Option<NonNull<wasm_store_t>>,
    bytes: *const wasm_byte_vec_t,
) -> Option<NonNull<wasm_module_t>> {
    // TODO: read config from store and use that to decide which compiler to use

    let byte_slice = if bytes.is_null() || (&*bytes).into_slice().is_none() {
        // TODO: error handling here
        return None;
    } else {
        (&*bytes).into_slice().unwrap()
    };

    let store_ptr: NonNull<Store> = store_ptr?.cast::<Store>();
    let store = store_ptr.as_ref();
    let module = if let Ok(module) = Module::deserialize(store, byte_slice) {
        module
    } else {
        // TODO: error handling here
        return None;
    };

    Some(NonNull::new_unchecked(Box::into_raw(Box::new(
        wasm_module_t {
            inner: Arc::new(module),
        },
    ))))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_serialize(
    module_ptr: *const wasm_module_t,
    out_ptr: *mut wasm_byte_vec_t,
) {
    let module: &wasm_module_t = &*module_ptr;
    let mut byte_vec = match module.inner.serialize() {
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

/// Opaque wrapper around `Store`
#[repr(C)]
pub struct wasm_store_t {}

#[no_mangle]
pub unsafe extern "C" fn wasm_store_new(
    wasm_engine_ptr: Option<NonNull<wasm_engine_t>>,
) -> Option<NonNull<wasm_store_t>> {
    let wasm_engine_ptr = wasm_engine_ptr?;
    let wasm_engine = wasm_engine_ptr.as_ref();
    let store = Store::new(wasm_engine.inner.clone());
    Some(NonNull::new_unchecked(
        Box::into_raw(Box::new(store)) as *mut wasm_store_t
    ))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_store_delete(wasm_store: Option<NonNull<wasm_store_t>>) {
    if let Some(s_inner) = wasm_store {
        // this should not leak memory:
        // we should double check it to make sure though
        let _: Box<Store> = Box::from_raw(s_inner.cast::<Store>().as_ptr());
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_as_extern(
    func_ptr: Option<NonNull<wasm_func_t>>,
) -> Option<NonNull<wasm_extern_t>> {
    let func_ptr = func_ptr?;
    let func = func_ptr.as_ref();

    let r#extern = Box::new(wasm_extern_t {
        instance: func.instance.clone(),
        inner: Extern::Function(func.inner.clone()),
    });
    Some(NonNull::new_unchecked(Box::into_raw(r#extern)))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_as_extern(
    global_ptr: Option<NonNull<wasm_global_t>>,
) -> Option<NonNull<wasm_extern_t>> {
    let global_ptr = global_ptr?;
    let global = global_ptr.as_ref();

    let r#extern = Box::new(wasm_extern_t {
        // update this if global does hold onto an `instance`
        instance: None,
        inner: Extern::Global(global.inner.clone()),
    });
    Some(NonNull::new_unchecked(Box::into_raw(r#extern)))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_as_extern(
    memory_ptr: Option<NonNull<wasm_memory_t>>,
) -> Option<NonNull<wasm_extern_t>> {
    let memory_ptr = memory_ptr?;
    let memory = memory_ptr.as_ref();

    let r#extern = Box::new(wasm_extern_t {
        // update this if global does hold onto an `instance`
        instance: None,
        inner: Extern::Memory(memory.inner.clone()),
    });
    Some(NonNull::new_unchecked(Box::into_raw(r#extern)))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_as_func(
    extern_ptr: Option<NonNull<wasm_extern_t>>,
) -> Option<NonNull<wasm_func_t>> {
    let extern_ptr = extern_ptr?;
    let r#extern = extern_ptr.as_ref();
    if let Extern::Function(f) = &r#extern.inner {
        let wasm_func = Box::new(wasm_func_t {
            inner: f.clone(),
            instance: r#extern.instance.clone(),
        });
        Some(NonNull::new_unchecked(Box::into_raw(wasm_func)))
    } else {
        None
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_as_global(
    extern_ptr: Option<NonNull<wasm_extern_t>>,
) -> Option<NonNull<wasm_global_t>> {
    let extern_ptr = extern_ptr?;
    let r#extern = extern_ptr.as_ref();
    if let Extern::Global(g) = &r#extern.inner {
        let wasm_global = Box::new(wasm_global_t { inner: g.clone() });
        Some(NonNull::new_unchecked(Box::into_raw(wasm_global)))
    } else {
        None
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_as_memory(
    extern_ptr: Option<NonNull<wasm_extern_t>>,
) -> Option<NonNull<wasm_memory_t>> {
    let extern_ptr = extern_ptr?;
    let r#extern = extern_ptr.as_ref();
    if let Extern::Memory(m) = &r#extern.inner {
        let wasm_memory = Box::new(wasm_memory_t { inner: m.clone() });
        Some(NonNull::new_unchecked(Box::into_raw(wasm_memory)))
    } else {
        None
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
    #[allow(dead_code)]
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

impl From<wasm_mutability_enum> for Mutability {
    fn from(other: wasm_mutability_enum) -> Self {
        match other {
            wasm_mutability_enum::WASM_CONST => Mutability::Const,
            wasm_mutability_enum::WASM_VAR => Mutability::Var,
        }
    }
}

#[allow(non_camel_case_types)]
pub type wasm_valkind_t = u8;

impl From<wasm_valkind_enum> for ValType {
    fn from(other: wasm_valkind_enum) -> Self {
        use wasm_valkind_enum::*;
        match other {
            WASM_I32 => ValType::I32,
            WASM_I64 => ValType::I64,
            WASM_F32 => ValType::F32,
            WASM_F64 => ValType::F64,
            WASM_ANYREF => todo!("WASM_ANYREF variant not yet implemented"),
            WASM_FUNCREF => todo!("WASM_FUNCREF variant not yet implemented"),
        }
    }
}

impl From<wasm_valtype_t> for ValType {
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
#[derive(Clone, Copy)]
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

impl Clone for wasm_val_t {
    fn clone(&self) -> Self {
        wasm_val_t {
            kind: self.kind,
            of: self.of.clone(),
        }
    }
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
pub unsafe extern "C" fn wasm_val_delete(val: Option<NonNull<wasm_val_t>>) {
    if let Some(v_inner) = val {
        // TODO: figure out where wasm_val is allocated first...
        let _ = Box::from_raw(v_inner.as_ptr());
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

impl TryFrom<wasm_val_t> for Val {
    type Error = &'static str;

    fn try_from(item: wasm_val_t) -> Result<Self, Self::Error> {
        (&item).try_into()
    }
}

impl TryFrom<&wasm_val_t> for Val {
    type Error = &'static str;

    fn try_from(item: &wasm_val_t) -> Result<Self, Self::Error> {
        Ok(match item.kind.try_into()? {
            wasm_valkind_enum::WASM_I32 => Val::I32(unsafe { item.of.int32_t }),
            wasm_valkind_enum::WASM_I64 => Val::I64(unsafe { item.of.int64_t }),
            wasm_valkind_enum::WASM_F32 => Val::F32(unsafe { item.of.float32_t }),
            wasm_valkind_enum::WASM_F64 => Val::F64(unsafe { item.of.float64_t }),
            wasm_valkind_enum::WASM_ANYREF => return Err("ANYREF not supported at this time"),
            wasm_valkind_enum::WASM_FUNCREF => return Err("FUNCREF not supported at this time"),
        })
    }
}

impl TryFrom<Val> for wasm_val_t {
    type Error = &'static str;

    fn try_from(item: Val) -> Result<Self, Self::Error> {
        wasm_val_t::try_from(&item)
    }
}

impl TryFrom<&Val> for wasm_val_t {
    type Error = &'static str;

    fn try_from(item: &Val) -> Result<Self, Self::Error> {
        Ok(match *item {
            Val::I32(v) => wasm_val_t {
                of: wasm_val_inner { int32_t: v },
                kind: wasm_valkind_enum::WASM_I32 as _,
            },
            Val::I64(v) => wasm_val_t {
                of: wasm_val_inner { int64_t: v },
                kind: wasm_valkind_enum::WASM_I64 as _,
            },
            Val::F32(v) => wasm_val_t {
                of: wasm_val_inner { float32_t: v },
                kind: wasm_valkind_enum::WASM_F32 as _,
            },
            Val::F64(v) => wasm_val_t {
                of: wasm_val_inner { float64_t: v },
                kind: wasm_valkind_enum::WASM_F64 as _,
            },
            Val::V128(_) => return Err("128bit SIMD types not yet supported in Wasm C API"),
            _ => todo!("Handle these values in TryFrom<Val> for wasm_val_t"),
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

#[repr(C)]
pub struct wasm_func_t {
    inner: Function,
    // this is how we ensure the instance stays alive
    instance: Option<Arc<Instance>>,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_new(
    store: Option<NonNull<wasm_store_t>>,
    ft: *const wasm_functype_t,
    callback: wasm_func_callback_t,
) -> Option<NonNull<wasm_func_t>> {
    // TODO: handle null pointers?
    let store_ptr = store?.cast::<Store>();
    let store = store_ptr.as_ref();
    let new_ft = wasm_functype_copy(NonNull::new(ft as *mut _))?;
    let func_sig = functype_to_real_type(new_ft);
    let num_rets = func_sig.results().len();
    let inner_callback = move |args: &[Val]| -> Result<Vec<Val>, RuntimeError> {
        let processed_args = args
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<wasm_val_t>, _>>()
            .expect("Argument conversion failed");

        let mut results = vec![
            wasm_val_t {
                kind: wasm_valkind_enum::WASM_I64 as _,
                of: wasm_val_inner { int64_t: 0 },
            };
            num_rets
        ];

        let _traps = callback(processed_args.as_ptr(), results.as_mut_ptr());
        // TODO: do something with `traps`

        let processed_results = results
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<Val>, _>>()
            .expect("Result conversion failed");
        Ok(processed_results)
    };
    let f = Function::new_dynamic(store, &func_sig, inner_callback);
    let wasm_func = Box::new(wasm_func_t {
        instance: None,
        inner: f,
    });
    Some(NonNull::new_unchecked(Box::into_raw(wasm_func)))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_new_with_env(
    _store: *mut wasm_store_t,
    ft: *const wasm_functype_t,
    callback: wasm_func_callback_with_env_t,
    env: c_void,
    finalizer: wasm_env_finalizer_t,
) -> *mut wasm_func_t {
    todo!("wasm_func_new_with_env")
    /*
    // TODO: handle null pointers?
    let new_ft = wasm_functype_copy(ft as *mut _);
    let func_sig = functype_to_real_type(new_ft);
    let wasm_func = Box::new(wasm_func_t {
        instance: None,
        inner: None,
        functype: func_sig,
    });
    Box::into_raw(wasm_func)
    */
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_delete(func: Option<NonNull<wasm_func_t>>) {
    if let Some(f_inner) = func {
        let _ = Box::from_raw(f_inner.as_ptr());
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_call(
    func: *const wasm_func_t,
    args: *const wasm_val_t,
    results: *mut wasm_val_t,
) -> Option<NonNull<wasm_trap_t>> {
    let func = &*func;

    let wasm_traps;
    let mut params = vec![];
    for (i, param) in func.inner.ty().params().iter().enumerate() {
        let arg = &(*args.add(i));

        match param {
            ValType::I32 => {
                if arg.kind != wasm_valkind_enum::WASM_I32 as u8 {
                    // todo: error handling
                    panic!("type mismatch!");
                }
                params.push(Val::I32(arg.of.int32_t));
            }
            ValType::I64 => {
                if arg.kind != wasm_valkind_enum::WASM_I64 as u8 {
                    // todo: error handling
                    panic!("type mismatch!");
                }
                params.push(Val::I64(arg.of.int64_t));
            }
            ValType::F32 => {
                if arg.kind != wasm_valkind_enum::WASM_F32 as u8 {
                    // todo: error handling
                    panic!("type mismatch!");
                }
                params.push(Val::F32(arg.of.float32_t));
            }
            ValType::F64 => {
                if arg.kind != wasm_valkind_enum::WASM_F64 as u8 {
                    // todo: error handling
                    panic!("type mismatch!");
                }
                params.push(Val::F64(arg.of.float64_t));
            }
            ValType::V128 => todo!("Handle v128 case in `wasm_func_call`"),
            _ => todo!("unhandled value cases in `wasm_func_call`"),
        }
    }

    match func.inner.call(&params) {
        Ok(wasm_results) => {
            for (i, actual_result) in wasm_results.iter().enumerate() {
                let result_loc = &mut (*results.add(i));
                match *actual_result {
                    Val::I32(v) => {
                        result_loc.of.int32_t = v;
                        result_loc.kind = wasm_valkind_enum::WASM_I32 as u8;
                    }
                    Val::I64(v) => {
                        result_loc.of.int64_t = v;
                        result_loc.kind = wasm_valkind_enum::WASM_I64 as u8;
                    }
                    Val::F32(v) => {
                        result_loc.of.float32_t = v;
                        result_loc.kind = wasm_valkind_enum::WASM_F32 as u8;
                    }
                    Val::F64(v) => {
                        result_loc.of.float64_t = v;
                        result_loc.kind = wasm_valkind_enum::WASM_F64 as u8;
                    }
                    Val::V128(_) => todo!("Handle v128 case in wasm_func_call"),
                    _ => todo!("handle other vals"),
                }
            }
            wasm_traps = None;
        }
        Err(e) => {
            wasm_traps = Some(NonNull::new_unchecked(Box::into_raw(Box::new(e)) as _));
        }
    }

    wasm_traps
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_param_arity(func: *const wasm_func_t) -> usize {
    let func = &*func;
    func.inner.ty().params().len()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_result_arity(func: *const wasm_func_t) -> usize {
    let func = &*func;
    func.inner.ty().results().len()
}

#[repr(C)]
pub struct wasm_global_t {
    // maybe needs to hold onto instance
    inner: Global,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_new(
    store_ptr: Option<NonNull<wasm_store_t>>,
    gt_ptr: *const wasm_globaltype_t,
    val_ptr: *const wasm_val_t,
) -> Option<NonNull<wasm_global_t>> {
    let gt = &*(gt_ptr as *const GlobalType);
    let val = &*val_ptr;
    let wasm_val = val.try_into().ok()?;
    let store_ptr: NonNull<Store> = store_ptr?.cast::<Store>();
    let store = store_ptr.as_ref();
    let global = if gt.mutability.is_mutable() {
        Global::new_mut(store, wasm_val)
    } else {
        Global::new(store, wasm_val)
    };

    Some(NonNull::new_unchecked(Box::into_raw(Box::new(
        wasm_global_t { inner: global },
    ))))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_delete(global: Option<NonNull<wasm_global_t>>) {
    if let Some(g_inner) = global {
        let _ = Box::from_raw(g_inner.as_ptr());
    }
}

// TODO: figure out if these should be deep or shallow copies
#[no_mangle]
pub unsafe extern "C" fn wasm_global_copy(
    global_ptr: *const wasm_global_t,
) -> NonNull<wasm_global_t> {
    let wasm_global = &*global_ptr;

    // do shallow copy

    NonNull::new_unchecked(Box::into_raw(Box::new(wasm_global_t {
        inner: wasm_global.inner.clone(),
    })))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_get(global_ptr: *const wasm_global_t, out: *mut wasm_val_t) {
    let wasm_global = &*global_ptr;
    let value = wasm_global.inner.get();
    *out = value.try_into().unwrap();
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_set(
    global_ptr: *mut wasm_global_t,
    val_ptr: *const wasm_val_t,
) {
    let wasm_global: &mut wasm_global_t = &mut *global_ptr;
    let val: &wasm_val_t = &*val_ptr;
    let value: Val = val.try_into().unwrap();
    wasm_global.inner.set(value);
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_same(
    global_ptr1: *const wasm_global_t,
    global_ptr2: *const wasm_global_t,
) -> bool {
    let wasm_global1 = &*global_ptr1;
    let wasm_global2 = &*global_ptr2;

    wasm_global1.inner.same(&wasm_global2.inner)
}

#[repr(C)]
pub struct wasm_memory_t {
    // maybe needs to hold onto instance
    inner: Memory,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_new(
    store_ptr: Option<NonNull<wasm_store_t>>,
    mt_ptr: *const wasm_memorytype_t,
) -> Option<NonNull<wasm_memory_t>> {
    let md = (&*(mt_ptr as *const MemoryType)).clone();
    let store_ptr: NonNull<Store> = store_ptr?.cast::<Store>();
    let store = store_ptr.as_ref();

    // TODO: report this error
    let memory = Memory::new(store, md).ok()?;
    Some(NonNull::new_unchecked(Box::into_raw(Box::new(
        wasm_memory_t { inner: memory },
    ))))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_delete(memory: Option<NonNull<wasm_memory_t>>) {
    if let Some(m_inner) = memory {
        let _ = Box::from_raw(m_inner.as_ptr());
    }
}

// TODO: figure out if these should be deep or shallow copies
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_copy(
    memory_ptr: *const wasm_memory_t,
) -> NonNull<wasm_memory_t> {
    let wasm_memory = &*memory_ptr;

    // do shallow copy

    NonNull::new_unchecked(Box::into_raw(Box::new(wasm_memory_t {
        inner: wasm_memory.inner.clone(),
    })))
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
    mem::transmute::<&[std::cell::Cell<u8>], &[u8]>(&memory.inner.view()[..]) as *const [u8]
        as *const u8 as *mut u8
}

// size in bytes
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_data_size(memory_ptr: *const wasm_memory_t) -> usize {
    let memory = &*memory_ptr;
    memory.inner.size().bytes().0
}

// size in pages
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_size(memory_ptr: *const wasm_memory_t) -> u32 {
    let memory = &*memory_ptr;
    memory.inner.size().0 as _
}

// delta is in pages
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_grow(memory_ptr: *mut wasm_memory_t, delta: u32) -> bool {
    let memory = &mut *memory_ptr;
    memory.inner.grow(Pages(delta)).is_ok()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_same(
    memory_ptr1: *const wasm_memory_t,
    memory_ptr2: *const wasm_memory_t,
) -> bool {
    let wasm_memory1 = &*memory_ptr1;
    let wasm_memory2 = &*memory_ptr2;

    wasm_memory1.inner.same(&wasm_memory2.inner)
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
pub unsafe extern "C" fn wasm_trap_delete(trap: Option<NonNull<wasm_trap_t>>) {
    if let Some(t_inner) = trap {
        let _ = Box::from_raw(t_inner.cast::<RuntimeError>().as_ptr());
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
    // this is how we ensure the instance stays alive
    instance: Option<Arc<Instance>>,
    inner: Extern,
}
wasm_declare_boxed_vec!(extern);

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct wasm_valtype_t {
    valkind: wasm_valkind_enum,
}

wasm_declare_vec!(valtype);

#[no_mangle]
pub extern "C" fn wasm_valtype_new(kind: wasm_valkind_t) -> Option<NonNull<wasm_valtype_t>> {
    let kind_enum = kind.try_into().ok()?;
    let valtype = wasm_valtype_t { valkind: kind_enum };
    let valtype_ptr = Box::new(valtype);
    unsafe { Some(NonNull::new_unchecked(Box::into_raw(valtype_ptr))) }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_valtype_delete(valtype: Option<NonNull<wasm_valtype_t>>) {
    if let Some(v_inner) = valtype {
        let _ = Box::from_raw(v_inner.as_ptr());
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

// opaque type wrapping `GlobalType`
#[repr(C)]
pub struct wasm_globaltype_t {}

wasm_declare_vec!(globaltype);

#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_new(
    // own
    valtype: Option<NonNull<wasm_valtype_t>>,
    mutability: wasm_mutability_t,
) -> Option<NonNull<wasm_globaltype_t>> {
    wasm_globaltype_new_inner(valtype?, mutability)
}

#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_delete(globaltype: Option<NonNull<wasm_globaltype_t>>) {
    if let Some(g_inner) = globaltype {
        let _ = Box::from_raw(g_inner.cast::<GlobalType>().as_ptr());
    }
}

unsafe fn wasm_globaltype_new_inner(
    // own
    valtype_ptr: NonNull<wasm_valtype_t>,
    mutability: wasm_mutability_t,
) -> Option<NonNull<wasm_globaltype_t>> {
    let me: wasm_mutability_enum = mutability.try_into().ok()?;
    let valtype = *valtype_ptr.as_ref();
    let gd = Box::new(GlobalType::new(valtype.into(), me.into()));
    wasm_valtype_delete(Some(valtype_ptr));

    Some(NonNull::new_unchecked(
        Box::into_raw(gd) as *mut wasm_globaltype_t
    ))
}

// opaque type wrapping `MemoryType`
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
) -> NonNull<wasm_memorytype_t> {
    let limits = *limits;
    let min_pages = Pages(limits.min as _);
    // TODO: investigate if `0` is in fact a sentinel value here
    let max_pages = if limits.max == 0 {
        None
    } else {
        Some(Pages(limits.max as _))
    };
    NonNull::new_unchecked(
        Box::into_raw(Box::new(MemoryType::new(min_pages, max_pages, false)))
            as *mut wasm_memorytype_t,
    )
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memorytype_delete(memorytype: *mut wasm_memorytype_t) {
    if !memorytype.is_null() {
        let _ = Box::from_raw(memorytype as *mut MemoryType);
    }
}

// TODO: fix memory leak
// this function leaks memory because the returned limits pointer is not owned
#[no_mangle]
pub unsafe extern "C" fn wasm_memorytype_limits(
    mt: *const wasm_memorytype_t,
) -> *const wasm_limits_t {
    let md = &*(mt as *const MemoryType);
    Box::into_raw(Box::new(wasm_limits_t {
        min: md.minimum.bytes().0 as _,
        max: md.maximum.map(|max| max.bytes().0 as _).unwrap_or(0),
    }))
}

// opaque type wrapping `Arc<FunctionType>`
#[repr(C)]
pub struct wasm_functype_t {}

wasm_declare_vec!(functype);

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_new(
    // own
    params: Option<NonNull<wasm_valtype_vec_t>>,
    // own
    results: Option<NonNull<wasm_valtype_vec_t>>,
) -> Option<NonNull<wasm_functype_t>> {
    wasm_functype_new_inner(params?, results?)
}

unsafe fn wasm_functype_new_inner(
    // own
    params: NonNull<wasm_valtype_vec_t>,
    // own
    results: NonNull<wasm_valtype_vec_t>,
) -> Option<NonNull<wasm_functype_t>> {
    let params = params.as_ref();
    let results = results.as_ref();
    let params: Vec<ValType> = params
        .into_slice()?
        .iter()
        .copied()
        .map(Into::into)
        .collect::<Vec<_>>();
    let results: Vec<ValType> = results
        .into_slice()?
        .iter()
        .copied()
        .map(Into::into)
        .collect::<Vec<_>>();

    let funcsig = Arc::new(FunctionType::new(params, results));
    Some(NonNull::new_unchecked(
        Arc::into_raw(funcsig) as *mut wasm_functype_t
    ))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_delete(arg: Option<NonNull<wasm_functype_t>>) {
    if let Some(arg_inner) = arg {
        let _ = Arc::from_raw(arg_inner.cast::<FunctionType>().as_ptr());
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_copy(
    arg: Option<NonNull<wasm_functype_t>>,
) -> Option<NonNull<wasm_functype_t>> {
    let funcsig = functype_to_real_type(arg?);
    let new_funcsig = Arc::clone(&funcsig);
    // don't free the original Arc
    mem::forget(funcsig);
    Some(NonNull::new_unchecked(
        Arc::into_raw(new_funcsig) as *mut wasm_functype_t
    ))
}

unsafe fn functype_to_real_type(arg: NonNull<wasm_functype_t>) -> Arc<FunctionType> {
    Arc::from_raw(arg.cast::<FunctionType>().as_ptr())
}

#[repr(C)]
pub struct wasm_frame_t {}

wasm_declare_vec!(frame);
