//! entrypoints for the standard C API

use std::convert::{TryFrom, TryInto};
use std::ffi::c_void;
use std::mem;
use std::ptr::{self, NonNull};
use std::slice;
use std::sync::Arc;

pub(crate) mod utils;
#[cfg(feature = "wasi")]
pub mod wasi;

// required due to really weird Rust resolution rules
// https://github.com/rust-lang/rust/issues/57966
use crate::c_try;

use crate::ordered_resolver::OrderedResolver;
use wasmer::{
    Engine, ExportType, Extern, ExternType, Function, FunctionType, Global, GlobalType, ImportType,
    Instance, Memory, MemoryType, Module, Mutability, Pages, RuntimeError, Store, Table, TableType,
    Val, ValType,
};
#[cfg(feature = "jit")]
use wasmer_engine_jit::JIT;
#[cfg(feature = "native")]
use wasmer_engine_native::Native;
#[cfg(feature = "object-file")]
use wasmer_engine_object_file::{ObjectFile, ObjectFileArtifact};

/// this can be a wasmer-specific type with wasmer-specific functions for manipulating it
#[repr(C)]
pub struct wasm_config_t {}

#[no_mangle]
pub extern "C" fn wasm_config_new() -> *mut wasm_config_t {
    todo!("wasm_config_new")
    //ptr::null_mut()
}

#[repr(C)]
pub struct wasm_engine_t {
    pub(crate) inner: Arc<dyn Engine + Send + Sync>,
}

// Compiler JIT
#[cfg(feature = "compiler")]
use wasmer_compiler::CompilerConfig;
#[cfg(feature = "compiler")]
fn get_default_compiler_config() -> Box<dyn CompilerConfig> {
    cfg_if! {
        if #[cfg(feature = "cranelift")] {
            Box::new(wasmer_compiler_cranelift::Cranelift::default())
        } else if #[cfg(feature = "llvm")] {
            Box::new(wasmer_compiler_llvm::LLVM::default())
        } else if #[cfg(feature = "singlepass")] {
            Box::new(wasmer_compiler_singlepass::Singlepass::default())
        } else {
            compile_error!("Please enable one of the compiler backends")
        }
    }
}

cfg_if! {
    if #[cfg(all(feature = "jit", feature = "compiler"))] {
        #[no_mangle]
        pub extern "C" fn wasm_engine_new() -> Box<wasm_engine_t> {
            let compiler_config: Box<dyn CompilerConfig> = get_default_compiler_config();
            let engine: Arc<dyn Engine + Send + Sync> = Arc::new(JIT::new(&*compiler_config).engine());
            Box::new(wasm_engine_t { inner: engine })
        }
    }
    else if #[cfg(feature = "jit")] {
        // Headless JIT
        #[no_mangle]
        pub extern "C" fn wasm_engine_new() -> Box<wasm_engine_t> {
            let engine: Arc<dyn Engine + Send + Sync> = Arc::new(JIT::headless().engine());
            Box::new(wasm_engine_t { inner: engine })
        }
    }
    else if #[cfg(all(feature = "native", feature = "compiler"))] {
        #[no_mangle]
        pub extern "C" fn wasm_engine_new() -> Box<wasm_engine_t> {
            let mut compiler_config: Box<dyn CompilerConfig> = get_default_compiler_config();
            let engine: Arc<dyn Engine + Send + Sync> = Arc::new(Native::new(&mut *compiler_config).engine());
            Box::new(wasm_engine_t { inner: engine })
        }
    }
    else if #[cfg(feature = "native")] {
        #[no_mangle]
        pub extern "C" fn wasm_engine_new() -> Box<wasm_engine_t> {
            let engine: Arc<dyn Engine + Send + Sync> = Arc::new(Native::headless().engine());
            Box::new(wasm_engine_t { inner: engine })
        }
    }
    else if #[cfg(all(feature = "object-file", feature = "compiler"))] {
        #[no_mangle]
        pub extern "C" fn wasm_engine_new() -> Box<wasm_engine_t> {
            let mut compiler_config: Box<dyn CompilerConfig> = get_default_compiler_config();
            let engine: Arc<dyn Engine + Send + Sync> = Arc::new(ObjectFile::new(&mut *compiler_config).engine());
            Box::new(wasm_engine_t { inner: engine })
        }
    }
    else if #[cfg(feature = "object-file")] {
        #[no_mangle]
        pub extern "C" fn wasm_engine_new() -> Box<wasm_engine_t> {
            let engine: Arc<dyn Engine + Send + Sync> = Arc::new(ObjectFile::headless().engine());
            Box::new(wasm_engine_t { inner: engine })
        }
    }
    else {
        #[no_mangle]
        pub extern "C" fn wasm_engine_new() -> Box<wasm_engine_t> {
            unimplemented!("The JITEngine is not attached");
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_engine_delete(_wasm_engine_address: Option<Box<wasm_engine_t>>) {}

#[no_mangle]
pub extern "C" fn wasm_engine_new_with_config(
    _config_ptr: *mut wasm_config_t,
) -> Box<wasm_engine_t> {
    wasm_engine_new()
}

#[repr(C)]
pub struct wasm_instance_t {
    inner: Arc<Instance>,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_new(
    store: Option<NonNull<wasm_store_t>>,
    module: &wasm_module_t,
    imports: *const *const wasm_extern_t,
    // own
    _traps: *mut *mut wasm_trap_t,
) -> Option<Box<wasm_instance_t>> {
    let wasm_module = &module.inner;
    let module_imports = wasm_module.imports();
    let module_import_count = module_imports.len();
    let imports = argument_import_iter(imports);
    let resolver: OrderedResolver = imports
        .map(|imp| &imp.inner)
        .take(module_import_count)
        .cloned()
        .collect();

    let instance = Arc::new(c_try!(Instance::new(wasm_module, &resolver)));
    Some(Box::new(wasm_instance_t { inner: instance }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_delete(_instance: Option<Box<wasm_instance_t>>) {}

// TODO: NOT part of the standard Wasm C API
#[no_mangle]
pub unsafe extern "C" fn wasm_instance_get_vmctx_ptr(instance: &wasm_instance_t) -> *mut c_void {
    instance.inner.vmctx_ptr() as _
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
    instance: &wasm_instance_t,
    // TODO: review types on wasm_declare_vec, handle the optional pointer part properly
    out: &mut wasm_extern_vec_t,
) {
    let instance = &instance.inner;
    let mut extern_vec = instance
        .exports
        .iter()
        .map(|(name, r#extern)| {
            let function = if let Extern::Function { .. } = r#extern {
                instance.exports.get_function(&name).ok().cloned()
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

    out.size = extern_vec.len();
    out.data = extern_vec.as_mut_ptr();
    // TODO: double check that the destructor will work correctly here
    mem::forget(extern_vec);
}

#[repr(C)]
pub struct wasm_module_t {
    pub(crate) inner: Arc<Module>,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_new(
    store_ptr: Option<NonNull<wasm_store_t>>,
    bytes: &wasm_byte_vec_t,
) -> Option<Box<wasm_module_t>> {
    // TODO: review lifetime of byte slice
    let wasm_byte_slice: &[u8] = slice::from_raw_parts_mut(bytes.data, bytes.size);
    let store_ptr: NonNull<Store> = store_ptr?.cast::<Store>();
    let store = store_ptr.as_ref();
    let module = c_try!(Module::from_binary(store, wasm_byte_slice));

    Some(Box::new(wasm_module_t {
        inner: Arc::new(module),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_delete(_module: Option<Box<wasm_module_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_exports(
    module: &wasm_module_t,
    out: &mut wasm_exporttype_vec_t,
) {
    let mut exports = module
        .inner
        .exports()
        .map(Into::into)
        .map(Box::new)
        .map(Box::into_raw)
        .collect::<Vec<*mut wasm_exporttype_t>>();
    exports.shrink_to_fit();

    debug_assert_eq!(exports.len(), exports.capacity());
    out.size = exports.len();
    out.data = exports.as_mut_ptr();
    mem::forget(exports);
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_imports(
    module: &wasm_module_t,
    out: &mut wasm_importtype_vec_t,
) {
    let mut imports = module
        .inner
        .imports()
        .map(Into::into)
        .map(Box::new)
        .map(Box::into_raw)
        .collect::<Vec<*mut wasm_importtype_t>>();
    imports.shrink_to_fit();

    debug_assert_eq!(imports.len(), imports.capacity());
    out.size = imports.len();
    out.data = imports.as_mut_ptr();
    mem::forget(imports);
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
    let module = c_try!(Module::deserialize(store, byte_slice));

    Some(NonNull::new_unchecked(Box::into_raw(Box::new(
        wasm_module_t {
            inner: Arc::new(module),
        },
    ))))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_serialize(
    module: &wasm_module_t,
    out_ptr: &mut wasm_byte_vec_t,
) {
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
    out_ptr.size = byte_vec.len();
    out_ptr.data = byte_vec.as_mut_ptr();
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
    let store = Store::new(&*wasm_engine.inner);
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
) -> Option<Box<wasm_extern_t>> {
    let func_ptr = func_ptr?;
    let func = func_ptr.as_ref();

    Some(Box::new(wasm_extern_t {
        instance: func.instance.clone(),
        inner: Extern::Function(func.inner.clone()),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_as_extern(
    global_ptr: Option<NonNull<wasm_global_t>>,
) -> Option<Box<wasm_extern_t>> {
    let global_ptr = global_ptr?;
    let global = global_ptr.as_ref();

    Some(Box::new(wasm_extern_t {
        // update this if global does hold onto an `instance`
        instance: None,
        inner: Extern::Global(global.inner.clone()),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_as_extern(
    memory_ptr: Option<NonNull<wasm_memory_t>>,
) -> Option<Box<wasm_extern_t>> {
    let memory_ptr = memory_ptr?;
    let memory = memory_ptr.as_ref();

    Some(Box::new(wasm_extern_t {
        // update this if global does hold onto an `instance`
        instance: None,
        inner: Extern::Memory(memory.inner.clone()),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_as_extern(
    table_ptr: Option<NonNull<wasm_table_t>>,
) -> Option<Box<wasm_extern_t>> {
    let table_ptr = table_ptr?;
    let table = table_ptr.as_ref();

    Some(Box::new(wasm_extern_t {
        // update this if global does hold onto an `instance`
        instance: None,
        inner: Extern::Table(table.inner.clone()),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_as_func(
    extern_ptr: Option<NonNull<wasm_extern_t>>,
) -> Option<Box<wasm_func_t>> {
    let extern_ptr = extern_ptr?;
    let r#extern = extern_ptr.as_ref();
    if let Extern::Function(f) = &r#extern.inner {
        Some(Box::new(wasm_func_t {
            inner: f.clone(),
            instance: r#extern.instance.clone(),
        }))
    } else {
        None
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_as_global(
    extern_ptr: Option<NonNull<wasm_extern_t>>,
) -> Option<Box<wasm_global_t>> {
    let extern_ptr = extern_ptr?;
    let r#extern = extern_ptr.as_ref();
    if let Extern::Global(g) = &r#extern.inner {
        Some(Box::new(wasm_global_t { inner: g.clone() }))
    } else {
        None
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_as_memory(
    extern_ptr: Option<NonNull<wasm_extern_t>>,
) -> Option<Box<wasm_memory_t>> {
    let extern_ptr = extern_ptr?;
    let r#extern = extern_ptr.as_ref();
    if let Extern::Memory(m) = &r#extern.inner {
        Some(Box::new(wasm_memory_t { inner: m.clone() }))
    } else {
        None
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_as_table(
    extern_ptr: Option<NonNull<wasm_extern_t>>,
) -> Option<Box<wasm_table_t>> {
    let extern_ptr = extern_ptr?;
    let r#extern = extern_ptr.as_ref();
    if let Extern::Table(t) = &r#extern.inner {
        Some(Box::new(wasm_table_t { inner: t.clone() }))
    } else {
        None
    }
}

#[allow(non_camel_case_types)]
pub type wasm_table_size_t = u32;

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

impl From<wasm_mutability_enum> for wasm_mutability_t {
    fn from(other: wasm_mutability_enum) -> Self {
        other as wasm_mutability_t
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

impl From<Mutability> for wasm_mutability_enum {
    fn from(other: Mutability) -> Self {
        match other {
            Mutability::Const => wasm_mutability_enum::WASM_CONST,
            Mutability::Var => wasm_mutability_enum::WASM_VAR,
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
            WASM_ANYREF => ValType::ExternRef,
            WASM_FUNCREF => ValType::FuncRef,
        }
    }
}

impl From<wasm_valtype_t> for ValType {
    fn from(other: wasm_valtype_t) -> Self {
        other.valkind.into()
    }
}

impl From<ValType> for wasm_valtype_t {
    fn from(other: ValType) -> Self {
        Self {
            valkind: other.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
#[repr(u8)]
pub enum wasm_valkind_enum {
    WASM_I32 = 0,
    WASM_I64 = 1,
    WASM_F32 = 2,
    WASM_F64 = 3,
    WASM_ANYREF = 128,
    WASM_FUNCREF = 129,
}

impl From<ValType> for wasm_valkind_enum {
    fn from(other: ValType) -> Self {
        match other {
            ValType::I32 => Self::WASM_I32,
            ValType::I64 => Self::WASM_I64,
            ValType::F32 => Self::WASM_F32,
            ValType::F64 => Self::WASM_F64,
            ValType::V128 => todo!("no v128 type in Wasm C API yet!"),
            ValType::ExternRef => Self::WASM_ANYREF,
            ValType::FuncRef => Self::WASM_FUNCREF,
        }
    }
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
pub unsafe extern "C" fn wasm_val_copy(out_ptr: *mut wasm_val_t, val: &wasm_val_t) {
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
    *mut c_void,
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
    ft: &wasm_functype_t,
    callback: wasm_func_callback_t,
) -> Option<Box<wasm_func_t>> {
    // TODO: handle null pointers?
    let store_ptr = store?.cast::<Store>();
    let store = store_ptr.as_ref();
    let func_sig = ft.sig();
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
    let f = Function::new(store, &func_sig, inner_callback);
    Some(Box::new(wasm_func_t {
        instance: None,
        inner: f,
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_new_with_env(
    store: Option<NonNull<wasm_store_t>>,
    ft: &wasm_functype_t,
    callback: wasm_func_callback_with_env_t,
    env: *mut c_void,
    finalizer: wasm_env_finalizer_t,
) -> Option<Box<wasm_func_t>> {
    // TODO: handle null pointers?
    let store_ptr = store?.cast::<Store>();
    let store = store_ptr.as_ref();
    let func_sig = ft.sig();
    let num_rets = func_sig.results().len();
    let inner_callback =
        move |env: &mut *mut c_void, args: &[Val]| -> Result<Vec<Val>, RuntimeError> {
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

            let _traps = callback(*env, processed_args.as_ptr(), results.as_mut_ptr());
            // TODO: do something with `traps`

            let processed_results = results
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<Val>, _>>()
                .expect("Result conversion failed");
            Ok(processed_results)
        };
    let f = Function::new_with_env(store, &func_sig, env, inner_callback);
    Some(Box::new(wasm_func_t {
        instance: None,
        inner: f,
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_delete(_func: Option<Box<wasm_func_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_call(
    func: &wasm_func_t,
    args: *const wasm_val_t,
    results: *mut wasm_val_t,
) -> Option<NonNull<wasm_trap_t>> {
    let num_params = func.inner.ty().params().len();
    let params: Vec<Val> = (0..num_params)
        .map(|i| (&(*args.add(i))).try_into())
        .collect::<Result<_, _>>()
        .ok()?;

    match func.inner.call(&params) {
        Ok(wasm_results) => {
            for (i, actual_result) in wasm_results.iter().enumerate() {
                let result_loc = &mut (*results.add(i));
                *result_loc = (&*actual_result).try_into().ok()?;
            }
            None
        }
        Err(e) => Some(NonNull::new_unchecked(Box::into_raw(Box::new(e)) as _)),
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_param_arity(func: &wasm_func_t) -> usize {
    func.inner.ty().params().len()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_result_arity(func: &wasm_func_t) -> usize {
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
    gt: &wasm_globaltype_t,
    val: &wasm_val_t,
) -> Option<Box<wasm_global_t>> {
    let gt = gt.as_globaltype();
    let wasm_val = val.try_into().ok()?;
    let store_ptr: NonNull<Store> = store_ptr?.cast::<Store>();
    let store = store_ptr.as_ref();
    let global = if gt.mutability.is_mutable() {
        Global::new_mut(store, wasm_val)
    } else {
        Global::new(store, wasm_val)
    };

    Some(Box::new(wasm_global_t { inner: global }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_delete(_global: Option<Box<wasm_global_t>>) {}

// TODO: figure out if these should be deep or shallow copies
#[no_mangle]
pub unsafe extern "C" fn wasm_global_copy(wasm_global: &wasm_global_t) -> Box<wasm_global_t> {
    // do shallow copy
    Box::new(wasm_global_t {
        inner: wasm_global.inner.clone(),
    })
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_get(wasm_global: &wasm_global_t, out: &mut wasm_val_t) {
    let value = wasm_global.inner.get();
    *out = value.try_into().unwrap();
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_set(wasm_global: &mut wasm_global_t, val: &wasm_val_t) {
    let value: Val = val.try_into().unwrap();
    wasm_global.inner.set(value);
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_same(
    wasm_global1: &wasm_global_t,
    wasm_global2: &wasm_global_t,
) -> bool {
    wasm_global1.inner.same(&wasm_global2.inner)
}

#[repr(C)]
pub struct wasm_memory_t {
    // maybe needs to hold onto instance
    pub(crate) inner: Memory,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_new(
    store_ptr: Option<NonNull<wasm_store_t>>,
    mt: &wasm_memorytype_t,
) -> Option<Box<wasm_memory_t>> {
    let md = mt.as_memorytype().clone();
    let store_ptr: NonNull<Store> = store_ptr?.cast::<Store>();
    let store = store_ptr.as_ref();

    let memory = c_try!(Memory::new(store, md));
    Some(Box::new(wasm_memory_t { inner: memory }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_delete(_memory: Option<Box<wasm_memory_t>>) {}

// TODO: figure out if these should be deep or shallow copies
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_copy(wasm_memory: &wasm_memory_t) -> Box<wasm_memory_t> {
    // do shallow copy
    Box::new(wasm_memory_t {
        inner: wasm_memory.inner.clone(),
    })
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_type(_memory_ptr: &wasm_memory_t) -> *mut wasm_memorytype_t {
    todo!("wasm_memory_type")
}

// get a raw pointer into bytes
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_data(memory: &mut wasm_memory_t) -> *mut u8 {
    mem::transmute::<&[std::cell::Cell<u8>], &[u8]>(&memory.inner.view()[..]) as *const [u8]
        as *const u8 as *mut u8
}

// size in bytes
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_data_size(memory: &wasm_memory_t) -> usize {
    memory.inner.size().bytes().0
}

// size in pages
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_size(memory: &wasm_memory_t) -> u32 {
    memory.inner.size().0 as _
}

// delta is in pages
#[no_mangle]
pub unsafe extern "C" fn wasm_memory_grow(memory: &mut wasm_memory_t, delta: u32) -> bool {
    memory.inner.grow(Pages(delta)).is_ok()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memory_same(
    wasm_memory1: &wasm_memory_t,
    wasm_memory2: &wasm_memory_t,
) -> bool {
    wasm_memory1.inner.same(&wasm_memory2.inner)
}

#[repr(C)]
pub struct wasm_table_t {
    // maybe needs to hold onto instance
    inner: Table,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_new(
    store_ptr: Option<NonNull<wasm_store_t>>,
    tt: &wasm_tabletype_t,
    init: *const wasm_ref_t,
) -> Option<Box<wasm_table_t>> {
    let tt = tt.as_tabletype().clone();
    let store_ptr: NonNull<Store> = store_ptr?.cast::<Store>();
    let store = store_ptr.as_ref();

    let init_val = todo!("get val from init somehow");

    let table = c_try!(Table::new(store, tt, init_val));
    Some(Box::new(wasm_table_t { inner: table }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_delete(_table: Option<Box<wasm_table_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_copy(wasm_table: &wasm_table_t) -> Box<wasm_table_t> {
    // do shallow copy
    Box::new(wasm_table_t {
        inner: wasm_table.inner.clone(),
    })
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_same(
    wasm_table1: &wasm_table_t,
    wasm_table2: &wasm_table_t,
) -> bool {
    wasm_table1.inner.same(&wasm_table2.inner)
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_size(wasm_table: &wasm_table_t) -> usize {
    wasm_table.inner.size() as _
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_grow(
    _wasm_table: &mut wasm_table_t,
    _delta: wasm_table_size_t,
    _init: *mut wasm_ref_t,
) -> bool {
    // TODO: maybe need to look at result to return `true`; also maybe report error here
    //wasm_table.inner.grow(delta, init).is_ok()
    todo!("Blocked on transforming ExternRef into a val type")
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
            pub unsafe extern "C" fn [<wasm_ $name _vec_new_empty>](out: *mut [<wasm_ $name _vec_t>]) {
                // TODO: actually implement this
                [<wasm_ $name _vec_new_uninitialized>](out, 0);
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

            // TODO: investigate possible memory leak on `init` (owned pointer)
            #[no_mangle]
            pub unsafe extern "C" fn [<wasm_ $name _vec_new>](out: *mut [<wasm_ $name _vec_t>], length: usize, init: *mut [<wasm_ $name _t>]) {
                let mut bytes: Vec<[<wasm_ $name _t>]> = Vec::with_capacity(length);
                for i in 0..length {
                    bytes.push(ptr::read(init.add(i)));
                }
                let pointer = bytes.as_mut_ptr();
                debug_assert!(bytes.len() == bytes.capacity());
                (*out).data = pointer;
                (*out).size = length;
                mem::forget(bytes);
            }

            #[no_mangle]
            pub unsafe extern "C" fn [<wasm_ $name _vec_new_uninitialized>](out: *mut [<wasm_ $name _vec_t>], length: usize) {
                let mut bytes: Vec<[<wasm_ $name _t>]> = Vec::with_capacity(length);
                let pointer = bytes.as_mut_ptr();
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

            // TODO: investigate possible memory leak on `init` (owned pointer)
            #[no_mangle]
            pub unsafe extern "C" fn [<wasm_ $name _vec_new>](out: *mut [<wasm_ $name _vec_t>], length: usize, init: *const *mut [<wasm_ $name _t>]) {
                let mut bytes: Vec<*mut [<wasm_ $name _t>]> = Vec::with_capacity(length);
                for i in 0..length {
                    bytes.push(*init.add(i));
                }
                let pointer = bytes.as_mut_ptr();
                debug_assert!(bytes.len() == bytes.capacity());
                (*out).data = pointer;
                (*out).size = length;
                mem::forget(bytes);
            }

            #[no_mangle]
            pub unsafe extern "C" fn [<wasm_ $name _vec_new_uninitialized>](out: *mut [<wasm_ $name _vec_t>], length: usize) {
                let mut bytes: Vec<*mut [<wasm_ $name _t>]> = Vec::with_capacity(length);
                let pointer = bytes.as_mut_ptr();
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

// opaque type over `ExternRef`?
#[allow(non_camel_case_types)]
pub struct wasm_ref_t;

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
    pub(crate) instance: Option<Arc<Instance>>,
    pub(crate) inner: Extern,
}
wasm_declare_boxed_vec!(extern);

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct wasm_valtype_t {
    valkind: wasm_valkind_enum,
}

impl Default for wasm_valtype_t {
    fn default() -> Self {
        Self {
            valkind: wasm_valkind_enum::WASM_I32,
        }
    }
}

wasm_declare_boxed_vec!(valtype);

#[no_mangle]
pub extern "C" fn wasm_valtype_new(kind: wasm_valkind_t) -> Option<Box<wasm_valtype_t>> {
    let kind_enum = kind.try_into().ok()?;
    let valtype = wasm_valtype_t { valkind: kind_enum };
    Some(Box::new(valtype))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_valtype_delete(_valtype: Option<Box<wasm_valtype_t>>) {}

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

#[derive(Clone, Debug)]
#[repr(transparent)]
#[allow(non_camel_case_types)]
pub struct wasm_globaltype_t {
    extern_: wasm_externtype_t,
}

impl wasm_globaltype_t {
    fn as_globaltype(&self) -> &GlobalType {
        if let ExternType::Global(ref g) = self.extern_.inner {
            g
        } else {
            unreachable!(
                "Data corruption detected: `wasm_globaltype_t` does not contain a `GlobalType`"
            );
        }
    }
}

wasm_declare_vec!(globaltype);

#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_new(
    // own
    valtype: Option<Box<wasm_valtype_t>>,
    mutability: wasm_mutability_t,
) -> Option<Box<wasm_globaltype_t>> {
    wasm_globaltype_new_inner(valtype?, mutability)
}

#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_delete(_globaltype: Option<Box<wasm_globaltype_t>>) {}

unsafe fn wasm_globaltype_new_inner(
    // own
    valtype: Box<wasm_valtype_t>,
    mutability: wasm_mutability_t,
) -> Option<Box<wasm_globaltype_t>> {
    let me: wasm_mutability_enum = mutability.try_into().ok()?;
    let gd = Box::new(wasm_globaltype_t {
        extern_: wasm_externtype_t {
            inner: ExternType::Global(GlobalType::new((*valtype).into(), me.into())),
        },
    });
    wasm_valtype_delete(Some(valtype));

    Some(gd)
}

#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_mutability(
    globaltype: &wasm_globaltype_t,
) -> wasm_mutability_t {
    let gt = globaltype.as_globaltype();
    wasm_mutability_enum::from(gt.mutability).into()
}

// TODO: fix memory leak
// this function leaks memory because the returned limits pointer is not owned
#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_content(
    globaltype: &wasm_globaltype_t,
) -> *const wasm_valtype_t {
    let gt = globaltype.as_globaltype();
    Box::into_raw(Box::new(gt.ty.into()))
}

#[derive(Clone, Debug)]
#[repr(C)]
#[allow(non_camel_case_types)]
pub struct wasm_tabletype_t {
    extern_: wasm_externtype_t,
}

wasm_declare_vec!(tabletype);

impl wasm_tabletype_t {
    fn as_tabletype(&self) -> &TableType {
        if let ExternType::Table(ref t) = self.extern_.inner {
            t
        } else {
            unreachable!(
                "Data corruption detected: `wasm_tabletype_t` does not contain a `TableType`"
            );
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_new(
    // own
    valtype: Box<wasm_valtype_t>,
    limits: &wasm_limits_t,
) -> Box<wasm_tabletype_t> {
    // TODO: investigate if `0` is in fact a sentinel value here
    let max_elements = if limits.max == 0 {
        None
    } else {
        Some(limits.max as _)
    };
    let out = Box::new(wasm_tabletype_t {
        extern_: wasm_externtype_t {
            inner: ExternType::Table(TableType::new(
                (*valtype).into(),
                limits.min as _,
                max_elements,
            )),
        },
    });
    wasm_valtype_delete(Some(valtype));

    out
}

// TODO: fix memory leak
// this function leaks memory because the returned limits pointer is not owned
#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_limits(
    tabletype: &wasm_tabletype_t,
) -> *const wasm_limits_t {
    let tt = tabletype.as_tabletype();
    Box::into_raw(Box::new(wasm_limits_t {
        min: tt.minimum as _,
        max: tt.maximum.unwrap_or(0),
    }))
}

// TODO: fix memory leak
// this function leaks memory because the returned limits pointer is not owned
#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_element(
    tabletype: &wasm_tabletype_t,
) -> *const wasm_valtype_t {
    let tt = tabletype.as_tabletype();

    Box::into_raw(Box::new(tt.ty.into()))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_delete(_tabletype: Option<Box<wasm_tabletype_t>>) {}

// opaque type wrapping `MemoryType`
#[derive(Clone, Debug)]
#[repr(transparent)]
#[allow(non_camel_case_types)]
pub struct wasm_memorytype_t {
    extern_: wasm_externtype_t,
}

impl wasm_memorytype_t {
    pub(crate) fn as_memorytype(&self) -> &MemoryType {
        if let ExternType::Memory(ref mt) = self.extern_.inner {
            mt
        } else {
            unreachable!(
                "Data corruption detected: `wasm_memorytype_t` does not contain a `MemoryType`"
            );
        }
    }
}

wasm_declare_vec!(memorytype);

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct wasm_limits_t {
    min: u32,
    max: u32,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memorytype_new(limits: &wasm_limits_t) -> Box<wasm_memorytype_t> {
    let min_pages = Pages(limits.min as _);
    // TODO: investigate if `0` is in fact a sentinel value here
    let max_pages = if limits.max == 0 {
        None
    } else {
        Some(Pages(limits.max as _))
    };
    Box::new(wasm_memorytype_t {
        extern_: wasm_externtype_t {
            inner: ExternType::Memory(MemoryType::new(min_pages, max_pages, false)),
        },
    })
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memorytype_delete(_memorytype: Option<Box<wasm_memorytype_t>>) {}

// TODO: fix memory leak
// this function leaks memory because the returned limits pointer is not owned
#[no_mangle]
pub unsafe extern "C" fn wasm_memorytype_limits(mt: &wasm_memorytype_t) -> *const wasm_limits_t {
    let md = mt.as_memorytype();
    Box::into_raw(Box::new(wasm_limits_t {
        min: md.minimum.bytes().0 as _,
        max: md.maximum.map(|max| max.bytes().0 as _).unwrap_or(0),
    }))
}

#[derive(Clone, Debug)]
#[allow(non_camel_case_types)]
#[repr(transparent)]
pub struct wasm_functype_t {
    extern_: wasm_externtype_t,
}

impl wasm_functype_t {
    pub(crate) fn sig(&self) -> &FunctionType {
        if let ExternType::Function(ref f) = self.extern_.inner {
            f
        } else {
            unreachable!("data corruption: `wasm_functype_t` does not contain a function")
        }
    }
}

wasm_declare_vec!(functype);

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_new(
    // own
    params: Option<NonNull<wasm_valtype_vec_t>>,
    // own
    results: Option<NonNull<wasm_valtype_vec_t>>,
) -> Option<Box<wasm_functype_t>> {
    wasm_functype_new_inner(params?, results?)
}

unsafe fn wasm_functype_new_inner(
    // own
    params: NonNull<wasm_valtype_vec_t>,
    // own
    results: NonNull<wasm_valtype_vec_t>,
) -> Option<Box<wasm_functype_t>> {
    let params = params.as_ref();
    let results = results.as_ref();
    let params: Vec<ValType> = params
        .into_slice()?
        .iter()
        .map(|&ptr| *ptr)
        .map(Into::into)
        .collect::<Vec<_>>();
    let results: Vec<ValType> = results
        .into_slice()?
        .iter()
        .map(|&ptr| *ptr)
        .map(Into::into)
        .collect::<Vec<_>>();

    let extern_ = wasm_externtype_t {
        inner: ExternType::Function(FunctionType::new(params, results)),
    };
    Some(Box::new(wasm_functype_t { extern_ }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_delete(_ft: Option<Box<wasm_functype_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_copy(
    arg: Option<NonNull<wasm_functype_t>>,
) -> Option<Box<wasm_functype_t>> {
    let arg = arg?;
    let funcsig = arg.as_ref();
    Some(Box::new(funcsig.clone()))
}

// TODO: fix memory leak
#[no_mangle]
pub unsafe extern "C" fn wasm_functype_params(ft: &wasm_functype_t) -> *const wasm_valtype_vec_t {
    let mut valtypes = ft
        .sig()
        .params()
        .iter()
        .cloned()
        .map(Into::into)
        .map(Box::new)
        .map(Box::into_raw)
        .collect::<Vec<*mut wasm_valtype_t>>();
    let out = Box::into_raw(Box::new(wasm_valtype_vec_t {
        size: valtypes.len(),
        data: valtypes.as_mut_ptr(),
    }));
    mem::forget(valtypes);
    out as *const _
}

// TODO: fix memory leak
#[no_mangle]
pub unsafe extern "C" fn wasm_functype_results(ft: &wasm_functype_t) -> *const wasm_valtype_vec_t {
    let mut valtypes = ft
        .sig()
        .results()
        .iter()
        .cloned()
        .map(Into::into)
        .map(Box::new)
        .map(Box::into_raw)
        .collect::<Vec<*mut wasm_valtype_t>>();
    let out = Box::into_raw(Box::new(wasm_valtype_vec_t {
        size: valtypes.len(),
        data: valtypes.as_mut_ptr(),
    }));
    mem::forget(valtypes);
    out as *const _
}

#[derive(Debug)]
#[repr(C)]
pub struct wasm_frame_t {}

wasm_declare_vec!(frame);

#[derive(Clone, Debug)]
#[allow(non_camel_case_types)]
#[repr(transparent)]
pub struct wasm_externtype_t {
    inner: ExternType,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_type(e: &wasm_extern_t) -> Box<wasm_externtype_t> {
    Box::new(wasm_externtype_t {
        inner: e.inner.ty(),
    })
}

#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_delete(_et: Option<Box<wasm_externtype_t>>) {}

impl From<ExternType> for wasm_externtype_t {
    fn from(other: ExternType) -> Self {
        Self { inner: other }
    }
}

impl From<&ExternType> for wasm_externtype_t {
    fn from(other: &ExternType) -> Self {
        other.clone().into()
    }
}

#[allow(non_camel_case_types)]
type wasm_externkind_t = u8;

#[allow(non_camel_case_types)]
#[repr(u8)]
pub enum wasm_externkind_enum {
    WASM_EXTERN_FUNC = 0,
    WASM_EXTERN_GLOBAL = 1,
    WASM_EXTERN_TABLE = 2,
    WASM_EXTERN_MEMORY = 3,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_kind(e: &wasm_extern_t) -> wasm_externkind_t {
    wasm_externkind_enum::from(e.inner.ty()) as wasm_externkind_t
}

impl From<ExternType> for wasm_externkind_enum {
    fn from(other: ExternType) -> Self {
        (&other).into()
    }
}
impl From<&ExternType> for wasm_externkind_enum {
    fn from(other: &ExternType) -> Self {
        match other {
            ExternType::Function(_) => Self::WASM_EXTERN_FUNC,
            ExternType::Global(_) => Self::WASM_EXTERN_GLOBAL,
            ExternType::Table(_) => Self::WASM_EXTERN_TABLE,
            ExternType::Memory(_) => Self::WASM_EXTERN_MEMORY,
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_kind(et: &wasm_externtype_t) -> wasm_externkind_t {
    wasm_externkind_enum::from(&et.inner) as wasm_externkind_t
}

#[derive(Debug, Clone, Error)]
#[error("failed to convert from `wasm_externtype_t`: {0}")]
pub struct ExternTypeConversionError(&'static str);
impl From<&'static str> for ExternTypeConversionError {
    fn from(other: &'static str) -> Self {
        Self(other)
    }
}

impl TryFrom<&'static wasm_externtype_t> for &'static wasm_functype_t {
    type Error = ExternTypeConversionError;
    fn try_from(other: &'static wasm_externtype_t) -> Result<Self, Self::Error> {
        if let ExternType::Function(_) = other.inner {
            Ok(unsafe { mem::transmute::<&'static wasm_externtype_t, Self>(other) })
        } else {
            Err(ExternTypeConversionError("Wrong type: expected function"))
        }
    }
}
impl TryFrom<&'static wasm_externtype_t> for &'static wasm_globaltype_t {
    type Error = ExternTypeConversionError;
    fn try_from(other: &'static wasm_externtype_t) -> Result<Self, Self::Error> {
        if let ExternType::Global(_) = other.inner {
            Ok(unsafe { mem::transmute::<&'static wasm_externtype_t, Self>(other) })
        } else {
            Err(ExternTypeConversionError("Wrong type: expected global"))
        }
    }
}
impl TryFrom<&'static wasm_externtype_t> for &'static wasm_memorytype_t {
    type Error = ExternTypeConversionError;
    fn try_from(other: &'static wasm_externtype_t) -> Result<Self, Self::Error> {
        if let ExternType::Memory(_) = other.inner {
            Ok(unsafe { mem::transmute::<&'static wasm_externtype_t, Self>(other) })
        } else {
            Err(ExternTypeConversionError("Wrong type: expected memory"))
        }
    }
}
impl TryFrom<&'static wasm_externtype_t> for &'static wasm_tabletype_t {
    type Error = ExternTypeConversionError;
    fn try_from(other: &'static wasm_externtype_t) -> Result<Self, Self::Error> {
        if let ExternType::Table(_) = other.inner {
            Ok(unsafe { mem::transmute::<&'static wasm_externtype_t, Self>(other) })
        } else {
            Err(ExternTypeConversionError("Wrong type: expected table"))
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_functype_const(
    et: &'static wasm_externtype_t,
) -> Option<&'static wasm_functype_t> {
    Some(c_try!(et.try_into()))
}
#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_functype(
    et: &'static wasm_externtype_t,
) -> Option<&'static wasm_functype_t> {
    Some(c_try!(et.try_into()))
}
#[no_mangle]
pub unsafe extern "C" fn wasm_functype_as_externtype_const(
    ft: &'static wasm_functype_t,
) -> &'static wasm_externtype_t {
    &ft.extern_
}
#[no_mangle]
pub unsafe extern "C" fn wasm_functype_as_externtype(
    ft: &'static wasm_functype_t,
) -> &'static wasm_externtype_t {
    &ft.extern_
}

#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_memorytype_const(
    et: &'static wasm_externtype_t,
) -> Option<&'static wasm_memorytype_t> {
    Some(c_try!(et.try_into()))
}
#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_memorytype(
    et: &'static wasm_externtype_t,
) -> Option<&'static wasm_memorytype_t> {
    Some(c_try!(et.try_into()))
}
#[no_mangle]
pub unsafe extern "C" fn wasm_memorytype_as_externtype_const(
    mt: &'static wasm_memorytype_t,
) -> &'static wasm_externtype_t {
    &mt.extern_
}
#[no_mangle]
pub unsafe extern "C" fn wasm_memorytype_as_externtype(
    mt: &'static wasm_memorytype_t,
) -> &'static wasm_externtype_t {
    &mt.extern_
}

#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_globaltype_const(
    et: &'static wasm_externtype_t,
) -> Option<&'static wasm_globaltype_t> {
    Some(c_try!(et.try_into()))
}
#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_globaltype(
    et: &'static wasm_externtype_t,
) -> Option<&'static wasm_globaltype_t> {
    Some(c_try!(et.try_into()))
}
#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_as_externtype_const(
    gt: &'static wasm_globaltype_t,
) -> &'static wasm_externtype_t {
    &gt.extern_
}
#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_as_externtype(
    gt: &'static wasm_globaltype_t,
) -> &'static wasm_externtype_t {
    &gt.extern_
}

#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_tabletype_const(
    et: &'static wasm_externtype_t,
) -> Option<&'static wasm_tabletype_t> {
    Some(c_try!(et.try_into()))
}
#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_tabletype(
    et: &'static wasm_externtype_t,
) -> Option<&'static wasm_tabletype_t> {
    Some(c_try!(et.try_into()))
}
#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_as_externtype_const(
    tt: &'static wasm_tabletype_t,
) -> &'static wasm_externtype_t {
    &tt.extern_
}
#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_as_externtype(
    tt: &'static wasm_tabletype_t,
) -> &'static wasm_externtype_t {
    &tt.extern_
}

#[allow(non_camel_case_types)]
type wasm_name_t = wasm_byte_vec_t;

#[repr(C)]
#[allow(non_camel_case_types)]
pub struct wasm_exporttype_t {
    name: NonNull<wasm_name_t>,
    extern_type: NonNull<wasm_externtype_t>,
}

wasm_declare_boxed_vec!(exporttype);

#[no_mangle]
pub extern "C" fn wasm_exporttype_new(
    name: NonNull<wasm_name_t>,
    extern_type: NonNull<wasm_externtype_t>,
) -> Box<wasm_exporttype_t> {
    Box::new(wasm_exporttype_t { name, extern_type })
}

#[no_mangle]
pub extern "C" fn wasm_exporttype_name(et: &'static wasm_exporttype_t) -> &'static wasm_name_t {
    unsafe { et.name.as_ref() }
}

#[no_mangle]
pub extern "C" fn wasm_exporttype_type(
    et: &'static wasm_exporttype_t,
) -> &'static wasm_externtype_t {
    unsafe { et.extern_type.as_ref() }
}

impl From<ExportType> for wasm_exporttype_t {
    fn from(other: ExportType) -> Self {
        (&other).into()
    }
}

impl From<&ExportType> for wasm_exporttype_t {
    fn from(other: &ExportType) -> Self {
        // TODO: double check that freeing String as `Vec<u8>` is valid
        let name = {
            let mut heap_str: Box<str> = other.name().to_string().into_boxed_str();
            let char_ptr = heap_str.as_mut_ptr();
            let str_len = heap_str.bytes().len();
            let name_inner = wasm_name_t {
                size: str_len,
                data: char_ptr,
            };
            Box::leak(heap_str);
            unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(name_inner))) }
        };

        let extern_type = {
            let extern_type: wasm_externtype_t = other.ty().into();
            unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(extern_type))) }
        };

        wasm_exporttype_t { name, extern_type }
    }
}

// TODO: improve ownership in `importtype_t` (can we safely use `Box<wasm_name_t>` here?)
#[repr(C)]
#[allow(non_camel_case_types)]
pub struct wasm_importtype_t {
    module: NonNull<wasm_name_t>,
    name: NonNull<wasm_name_t>,
    extern_type: NonNull<wasm_externtype_t>,
}

wasm_declare_boxed_vec!(importtype);

#[no_mangle]
pub extern "C" fn wasm_importtype_new(
    module: NonNull<wasm_name_t>,
    name: NonNull<wasm_name_t>,
    extern_type: NonNull<wasm_externtype_t>,
) -> Box<wasm_importtype_t> {
    Box::new(wasm_importtype_t {
        name,
        module,
        extern_type,
    })
}

#[no_mangle]
pub extern "C" fn wasm_importtype_module(et: &'static wasm_importtype_t) -> &'static wasm_name_t {
    unsafe { et.module.as_ref() }
}

#[no_mangle]
pub extern "C" fn wasm_importtype_name(et: &'static wasm_importtype_t) -> &'static wasm_name_t {
    unsafe { et.name.as_ref() }
}

#[no_mangle]
pub extern "C" fn wasm_importtype_type(
    et: &'static wasm_importtype_t,
) -> &'static wasm_externtype_t {
    unsafe { et.extern_type.as_ref() }
}

impl From<ImportType> for wasm_importtype_t {
    fn from(other: ImportType) -> Self {
        (&other).into()
    }
}

impl From<&ImportType> for wasm_importtype_t {
    fn from(other: &ImportType) -> Self {
        // TODO: double check that freeing String as `Vec<u8>` is valid
        let name = {
            let mut heap_str: Box<str> = other.name().to_string().into_boxed_str();
            let char_ptr = heap_str.as_mut_ptr();
            let str_len = heap_str.bytes().len();
            let name_inner = wasm_name_t {
                size: str_len,
                data: char_ptr,
            };
            Box::leak(heap_str);
            unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(name_inner))) }
        };

        // TODO: double check that freeing String as `Vec<u8>` is valid
        let module = {
            let mut heap_str: Box<str> = other.module().to_string().into_boxed_str();
            let char_ptr = heap_str.as_mut_ptr();
            let str_len = heap_str.bytes().len();
            let name_inner = wasm_name_t {
                size: str_len,
                data: char_ptr,
            };
            Box::leak(heap_str);
            unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(name_inner))) }
        };

        let extern_type = {
            let extern_type: wasm_externtype_t = other.ty().into();
            unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(extern_type))) }
        };

        wasm_importtype_t {
            name,
            module,
            extern_type,
        }
    }
}
