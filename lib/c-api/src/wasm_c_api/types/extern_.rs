use super::super::externals::wasm_extern_t;
use super::{
    wasm_functype_t, wasm_globaltype_t, wasm_memorytype_t, wasm_tabletype_t, WasmFunctionType,
    WasmGlobalType, WasmMemoryType, WasmTableType, WasmTagType,
};
use std::convert::{TryFrom, TryInto};
use std::mem;
use thiserror::Error;
use wasmer_api::ExternType;

#[allow(non_camel_case_types)]
pub type wasm_externkind_t = u8;

#[allow(non_camel_case_types)]
#[repr(u8)]
pub enum wasm_externkind_enum {
    WASM_EXTERN_FUNC = 0,
    WASM_EXTERN_GLOBAL = 1,
    WASM_EXTERN_TABLE = 2,
    WASM_EXTERN_MEMORY = 3,
    WASM_EXTERN_TAG = 4,
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
            ExternType::Tag(_) => Self::WASM_EXTERN_TAG,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum WasmExternType {
    Function(WasmFunctionType),
    Global(WasmGlobalType),
    Table(WasmTableType),
    Memory(WasmMemoryType),
    // No support for eh in the C-API yet.
    #[allow(unused)]
    Tag(WasmTagType),
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone)]
pub struct wasm_externtype_t {
    pub(crate) inner: WasmExternType,
}

impl wasm_externtype_t {
    pub(crate) fn new(extern_type: ExternType) -> Self {
        Self {
            inner: match extern_type {
                ExternType::Function(function_type) => {
                    WasmExternType::Function(WasmFunctionType::new(function_type))
                }
                ExternType::Global(global_type) => {
                    WasmExternType::Global(WasmGlobalType::new(global_type))
                }
                ExternType::Table(table_type) => {
                    WasmExternType::Table(WasmTableType::new(table_type))
                }
                ExternType::Memory(memory_type) => {
                    WasmExternType::Memory(WasmMemoryType::new(memory_type))
                }
                ExternType::Tag(tag_type) => WasmExternType::Tag(WasmTagType::new(tag_type)),
            },
        }
    }
}

impl From<ExternType> for wasm_externtype_t {
    fn from(extern_type: ExternType) -> Self {
        Self::new(extern_type)
    }
}

impl From<&ExternType> for wasm_externtype_t {
    fn from(other: &ExternType) -> Self {
        other.clone().into()
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_type(r#extern: &wasm_extern_t) -> Box<wasm_externtype_t> {
    Box::new(wasm_externtype_t::new(r#extern.ty()))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_extern_kind(r#extern: &wasm_extern_t) -> wasm_externkind_t {
    wasm_externkind_enum::from(r#extern.ty()) as wasm_externkind_t
}

#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_delete(_extern_type: Option<Box<wasm_externtype_t>>) {}

#[no_mangle]
pub extern "C" fn wasm_externtype_copy(extern_type: &wasm_externtype_t) -> Box<wasm_externtype_t> {
    Box::new(extern_type.clone())
}

#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_kind(
    extern_type: &wasm_externtype_t,
) -> wasm_externkind_t {
    (match extern_type.inner {
        WasmExternType::Function(_) => wasm_externkind_enum::WASM_EXTERN_FUNC,
        WasmExternType::Global(_) => wasm_externkind_enum::WASM_EXTERN_GLOBAL,
        WasmExternType::Table(_) => wasm_externkind_enum::WASM_EXTERN_TABLE,
        WasmExternType::Memory(_) => wasm_externkind_enum::WASM_EXTERN_MEMORY,
        WasmExternType::Tag(_) => wasm_externkind_enum::WASM_EXTERN_TAG,
    }) as wasm_externkind_t
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
        if let WasmExternType::Function(_) = other.inner {
            Ok(unsafe { mem::transmute::<&'static wasm_externtype_t, Self>(other) })
        } else {
            Err(ExternTypeConversionError("Wrong type: expected function"))
        }
    }
}

impl TryFrom<&'static wasm_externtype_t> for &'static wasm_globaltype_t {
    type Error = ExternTypeConversionError;

    fn try_from(other: &'static wasm_externtype_t) -> Result<Self, Self::Error> {
        if let WasmExternType::Global(_) = other.inner {
            Ok(unsafe { mem::transmute::<&'static wasm_externtype_t, Self>(other) })
        } else {
            Err(ExternTypeConversionError("Wrong type: expected global"))
        }
    }
}

impl TryFrom<&'static wasm_externtype_t> for &'static wasm_tabletype_t {
    type Error = ExternTypeConversionError;

    fn try_from(other: &'static wasm_externtype_t) -> Result<Self, Self::Error> {
        if let WasmExternType::Table(_) = other.inner {
            Ok(unsafe { mem::transmute::<&'static wasm_externtype_t, Self>(other) })
        } else {
            Err(ExternTypeConversionError("Wrong type: expected table"))
        }
    }
}

impl TryFrom<&'static wasm_externtype_t> for &'static wasm_memorytype_t {
    type Error = ExternTypeConversionError;

    fn try_from(other: &'static wasm_externtype_t) -> Result<Self, Self::Error> {
        if let WasmExternType::Memory(_) = other.inner {
            Ok(unsafe { mem::transmute::<&'static wasm_externtype_t, Self>(other) })
        } else {
            Err(ExternTypeConversionError("Wrong type: expected memory"))
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_functype_const(
    extern_type: &'static wasm_externtype_t,
) -> Option<&'static wasm_functype_t> {
    Some(c_try!(extern_type.try_into()))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_functype(
    extern_type: &'static wasm_externtype_t,
) -> Option<&'static wasm_functype_t> {
    Some(c_try!(extern_type.try_into()))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_as_externtype_const(
    function_type: &'static wasm_functype_t,
) -> &'static wasm_externtype_t {
    &function_type.extern_type
}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_as_externtype(
    function_type: &'static wasm_functype_t,
) -> &'static wasm_externtype_t {
    &function_type.extern_type
}

#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_globaltype_const(
    extern_type: &'static wasm_externtype_t,
) -> Option<&'static wasm_globaltype_t> {
    Some(c_try!(extern_type.try_into()))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_globaltype(
    extern_type: &'static wasm_externtype_t,
) -> Option<&'static wasm_globaltype_t> {
    Some(c_try!(extern_type.try_into()))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_as_externtype_const(
    global_type: &'static wasm_globaltype_t,
) -> &'static wasm_externtype_t {
    &global_type.extern_type
}

#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_as_externtype(
    global_type: &'static wasm_globaltype_t,
) -> &'static wasm_externtype_t {
    &global_type.extern_type
}

#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_tabletype_const(
    extern_type: &'static wasm_externtype_t,
) -> Option<&'static wasm_tabletype_t> {
    Some(c_try!(extern_type.try_into()))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_tabletype(
    extern_type: &'static wasm_externtype_t,
) -> Option<&'static wasm_tabletype_t> {
    Some(c_try!(extern_type.try_into()))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_as_externtype_const(
    table_type: &'static wasm_tabletype_t,
) -> &'static wasm_externtype_t {
    &table_type.extern_type
}

#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_as_externtype(
    table_type: &'static wasm_tabletype_t,
) -> &'static wasm_externtype_t {
    &table_type.extern_type
}

#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_memorytype_const(
    extern_type: &'static wasm_externtype_t,
) -> Option<&'static wasm_memorytype_t> {
    Some(c_try!(extern_type.try_into()))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_memorytype(
    extern_type: &'static wasm_externtype_t,
) -> Option<&'static wasm_memorytype_t> {
    Some(c_try!(extern_type.try_into()))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memorytype_as_externtype_const(
    memory_type: &'static wasm_memorytype_t,
) -> &'static wasm_externtype_t {
    &memory_type.extern_type
}

#[no_mangle]
pub unsafe extern "C" fn wasm_memorytype_as_externtype(
    memory_type: &'static wasm_memorytype_t,
) -> &'static wasm_externtype_t {
    &memory_type.extern_type
}
