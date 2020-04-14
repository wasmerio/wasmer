//! entrypoints for the standard C API
//! TODO: come up with a better name than `c_api`.

use std::convert::{TryFrom, TryInto};
use std::ffi::c_void;
use std::mem;
use std::ptr;
use std::slice;
use std::sync::Arc;

use wasmer::compiler::compile;
use wasmer::import::{ImportObject, LikeNamespace, Namespace};
use wasmer::module::Module;
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
pub struct wasm_instance_t {}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_new(
    _store: *mut wasm_store_t,
    module: *const wasm_module_t,
    imports: *const *const wasm_extern_t,
    // own
    _traps: *mut *mut wasm_trap_t,
) -> *mut wasm_instance_t {
    let module = &*(module as *mut Module);
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
            (wasm::ExternDescriptor::Global(_), wasm::Export::Global(_)) => todo!("global"),
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

    let instance = Box::new(
        module
            .instantiate(&import_object)
            .expect("failed to instantiate: TODO handle this error"),
    );
    Box::into_raw(instance) as *mut wasm_instance_t
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_delete(instance: *mut wasm_instance_t) {
    if !instance.is_null() {
        let _ = Box::from_raw(instance as *mut wasm::Instance);
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
unsafe fn argument_import_iter(imports: *const *const wasm_extern_t) -> CArrayIter<wasm_extern_t> {
    CArrayIter::new(imports).expect("Could not create iterator over imports argument")
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_exports(
    instance: *const wasm_instance_t,
    // TODO: review types on wasm_declare_vec, handle the optional pointer part properly
    out: *mut wasm_extern_vec_t,
) {
    let instance = &*(instance as *const wasm::Instance);
    // TODO: review name, does `into_iter` imply taking ownership?
    let mut extern_vec = instance
        .exports
        .into_iter()
        .map(|(_, export)| Box::into_raw(Box::new(wasm_extern_t { export })))
        .collect::<Vec<*mut wasm_extern_t>>();
    extern_vec.shrink_to_fit();

    (*out).size = extern_vec.len();
    (*out).data = extern_vec.as_mut_ptr();
    // TODO: double check that the destructor will work correctly here
    mem::forget(extern_vec);
}

// opaque wrapper around `*mut Module`
#[repr(C)]
pub struct wasm_module_t;

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

    Box::into_raw(Box::new(module)) as *mut wasm_module_t
}

#[no_mangle]
pub extern "C" fn wasm_module_delete(module: *mut wasm_module_t) {
    if !module.is_null() {
        unsafe { Box::from_raw(module as *mut Module) };
    }
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
            let export = Box::new(wasm::Export::Function {
                func: wasm::FuncPointer::new(callback as *const _),
                // TODO: figure out how to use `wasm::Context` correctly here
                ctx: wasm::Context::Internal,
                signature: Arc::clone(&func.functype),
            });

            Box::into_raw(export) as *mut wasm_extern_t
        }
    }
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

#[allow(non_camel_case_types)]
pub type wasm_valkind_t = u8;

/// Converts the numeric `wasm_valkind_t` into the structural type used by
/// Wasmer'`s internals: [`wasm::Type`].
fn valkind_to_type(vk: wasm_valkind_t) -> Option<wasm::Type> {
    Some(match vk {
        0 => wasm::Type::I32,
        1 => wasm::Type::I64,
        2 => wasm::Type::F32,
        3 => wasm::Type::F64,
        128 => todo!("WASM_ANYREF variant not yet implemented"),
        129 => todo!("WASM_FUNCREF variant not yet implemented"),
        _ => return None,
    })
}

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

    /*let mut params = vec![];
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
    }*/

    // TODO: investigate constructing a dynfunc from func.callback
    let wasm_traps = match func.callback {
        CallbackType::WithoutEnv(fp) => fp(args, results),
        _ => panic!("wat"),
    };

    wasm_traps

    /*
    for (i, actual_result) in wasm_results.iter().enumerate() {
        let result_loc = &mut (*results.add(i));
        match actual_result {
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

    //let args =
    ptr::null_mut()*/
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
            pub extern "C" fn [<wasm_ $name _vec_new_uninitialized>](out: *mut [<wasm_ $name _vec_t>], length: usize) {
                // TODO: actually implement this
                [<wasm_ $name _vec_new>](out, length);
            }

            #[no_mangle]
            pub extern "C" fn [<wasm_ $name _vec_new_empty>](out: *mut [<wasm_ $name _vec_t>]) {
                // TODO: actually implement this
                [<wasm_ $name _vec_new>](out, 0);
            }

            #[no_mangle]
            pub extern "C" fn [<wasm_ $name _vec_delete>](ptr: *mut [<wasm_ $name _vec_t>]) {
                unsafe {
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
            pub extern "C" fn [<wasm_ $name _vec_new>](out: *mut [<wasm_ $name _vec_t>], length: usize, /* TODO: this arg count is wrong)*/) {
                let mut bytes: Vec<[<wasm_ $name _t>]> = Vec::with_capacity(length);
                let pointer = bytes.as_mut_ptr();
                debug_assert!(bytes.len() == bytes.capacity());
                unsafe {
                    (*out).data = pointer;
                    (*out).size = length;
                };
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
            pub extern "C" fn [<wasm_ $name _vec_new>](out: *mut [<wasm_ $name _vec_t>], length: usize, /* TODO: this arg count is wrong)*/) {
                let mut bytes: Vec<*mut [<wasm_ $name _t>]> = Vec::with_capacity(length);
                let pointer = bytes.as_mut_ptr();
                debug_assert!(bytes.len() == bytes.capacity());
                unsafe {
                    (*out).data = pointer;
                    (*out).size = length;
                };
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

macro_rules! wasm_declare_ref {
    ($name:ident) => {
        wasm_declare_ref_base!($name);

        paste::item! {
            #[no_mangle]
            pub extern "C" fn [<wasm_ $name _as_ref>](_arg: *mut [<wasm_ $name _t>]) -> *mut wasm_ref_t {
                todo!("in {}", stringify!([<wasm_ $name _as_ref>]));
                //ptr::null_mut()
            }

            #[no_mangle]
            pub extern "C" fn [<wasm_ref_as_ $name>](_ref: *mut wasm_ref_t ) -> *mut [<wasm_ $name _t>] {
                todo!("in {}", stringify!([<wasm_ref_as_ $name>]));
                //ptr::null_mut()
            }

            #[no_mangle]
            pub extern "C" fn [<wasm_ $name _as_ref_const>](_arg: *const [<wasm_ $name _t>]) -> *const wasm_ref_t {
                todo!("in {}", stringify!([<wasm_ $name _as_ref_const>]));
                //ptr::null()
            }

            #[no_mangle]
            pub extern "C" fn [<wasm_ref_as_ $name _const>](_ref: *const wasm_ref_t ) -> *const [<wasm_ $name _t>] {
                todo!("in {}", stringify!([<wasm_ref_as_ $name _const>]));
                //ptr::null_mut()
            }
        }
    };
}

// TODO: inline this macro
/*
macro_rules! wasm_declare_type {
    ($name:ident) => {
        wasm_declare_own!($name);
        wasm_declare_vec!($name);
        paste::item! {
            #[no_mangle]
            pub extern "C" fn [<wasm_ $name _copy>](_arg: *mut [<wasm_ $name _t>]) -> *mut [<wasm_ $name _t>] {
                todo!("in {}", stringify!([<wasm_ $name _copy>]));
                //ptr::null_mut()
            }
        }
    };
}
*/

#[allow(non_camel_case_types)]
pub type wasm_byte_t = u8;
wasm_declare_vec!(byte);

wasm_declare_ref_base!(ref);
wasm_declare_ref!(trap);

#[repr(C)]
pub struct wasm_extern_t {
    export: wasm::Export,
}
wasm_declare_boxed_vec!(extern);

#[repr(C)]
pub struct wasm_valtype_t {
    valkind: wasm_valkind_t,
}

wasm_declare_vec!(valtype);

#[no_mangle]
pub extern "C" fn wasm_valtype_new(kind: wasm_valkind_t) -> *mut wasm_valtype_t {
    let valtype = wasm_valtype_t { valkind: kind };
    let valtype_ptr = Box::new(valtype);
    Box::into_raw(valtype_ptr)
}

#[no_mangle]
pub extern "C" fn wasm_valtype_delete(valtype: *mut wasm_valtype_t) {
    if !valtype.is_null() {
        let _ = unsafe { Box::from_raw(valtype) };
    }
}

#[no_mangle]
pub extern "C" fn wasm_valtype_kind(valtype: *const wasm_valtype_t) -> wasm_valkind_t {
    if valtype.is_null() {
        // TODO: handle error
        panic!("wasm_valtype_kind: argument is null pointer");
    }
    unsafe {
        return (*valtype).valkind;
    }
}

//wasm_declare_ref!(trap);
//wasm_declare_ref!(foreign);

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
        .map(|param| valkind_to_type(param.valkind))
        .collect::<Option<Vec<_>>>()?;
    let results: Vec<wasm::Type> = results
        .into_slice()?
        .iter()
        .map(|param| valkind_to_type(param.valkind))
        .collect::<Option<Vec<_>>>()?;

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
pub extern "C" fn wasm_functype_copy(arg: *mut wasm_functype_t) -> *mut wasm_functype_t {
    if !arg.is_null() {
        unsafe {
            let funcsig = functype_to_real_type(arg);
            let new_funcsig = Arc::clone(&funcsig);
            // don't free the original Arc
            mem::forget(funcsig);
            Arc::into_raw(new_funcsig) as *mut wasm_functype_t
        }
    } else {
        ptr::null_mut()
    }
}

unsafe fn functype_to_real_type(arg: *mut wasm_functype_t) -> Arc<wasm::FuncSig> {
    Arc::from_raw(arg as *mut wasm::FuncSig)
}
