use super::super::externals::wasm_extern_t;
use super::{wasm_functype_t, wasm_globaltype_t, wasm_memorytype_t, wasm_tabletype_t};
use std::convert::{TryFrom, TryInto};
use std::mem;
use thiserror::Error;
use wasmer::ExternType;

/// cbindgen:ignore
#[allow(non_camel_case_types)]
#[derive(Clone, Debug)]
pub struct wasm_externtype_t {
    pub(crate) inner: ExternType,
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_extern_type(e: &wasm_extern_t) -> Box<wasm_externtype_t> {
    Box::new(wasm_externtype_t {
        inner: e.inner.ty(),
    })
}

/// cbindgen:ignore
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

/// cbindgen:ignore
#[allow(non_camel_case_types)]
type wasm_externkind_t = u8;

/// cbindgen:ignore
#[allow(non_camel_case_types)]
#[repr(u8)]
pub enum wasm_externkind_enum {
    WASM_EXTERN_FUNC = 0,
    WASM_EXTERN_GLOBAL = 1,
    WASM_EXTERN_TABLE = 2,
    WASM_EXTERN_MEMORY = 3,
}

/// cbindgen:ignore
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

/// cbindgen:ignore
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

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_functype_const(
    et: &'static wasm_externtype_t,
) -> Option<&'static wasm_functype_t> {
    Some(c_try!(et.try_into()))
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_functype(
    et: &'static wasm_externtype_t,
) -> Option<&'static wasm_functype_t> {
    Some(c_try!(et.try_into()))
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_functype_as_externtype_const(
    ft: &'static wasm_functype_t,
) -> &'static wasm_externtype_t {
    &ft.extern_
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_functype_as_externtype(
    ft: &'static wasm_functype_t,
) -> &'static wasm_externtype_t {
    &ft.extern_
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_memorytype_const(
    et: &'static wasm_externtype_t,
) -> Option<&'static wasm_memorytype_t> {
    Some(c_try!(et.try_into()))
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_memorytype(
    et: &'static wasm_externtype_t,
) -> Option<&'static wasm_memorytype_t> {
    Some(c_try!(et.try_into()))
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_memorytype_as_externtype_const(
    mt: &'static wasm_memorytype_t,
) -> &'static wasm_externtype_t {
    &mt.extern_
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_memorytype_as_externtype(
    mt: &'static wasm_memorytype_t,
) -> &'static wasm_externtype_t {
    &mt.extern_
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_globaltype_const(
    et: &'static wasm_externtype_t,
) -> Option<&'static wasm_globaltype_t> {
    Some(c_try!(et.try_into()))
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_globaltype(
    et: &'static wasm_externtype_t,
) -> Option<&'static wasm_globaltype_t> {
    Some(c_try!(et.try_into()))
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_as_externtype_const(
    gt: &'static wasm_globaltype_t,
) -> &'static wasm_externtype_t {
    &gt.extern_
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_as_externtype(
    gt: &'static wasm_globaltype_t,
) -> &'static wasm_externtype_t {
    &gt.extern_
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_tabletype_const(
    et: &'static wasm_externtype_t,
) -> Option<&'static wasm_tabletype_t> {
    Some(c_try!(et.try_into()))
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_externtype_as_tabletype(
    et: &'static wasm_externtype_t,
) -> Option<&'static wasm_tabletype_t> {
    Some(c_try!(et.try_into()))
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_as_externtype_const(
    tt: &'static wasm_tabletype_t,
) -> &'static wasm_externtype_t {
    &tt.extern_
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_as_externtype(
    tt: &'static wasm_tabletype_t,
) -> &'static wasm_externtype_t {
    &tt.extern_
}
