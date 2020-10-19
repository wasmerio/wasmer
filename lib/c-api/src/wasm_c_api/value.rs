use super::types::{wasm_ref_t, wasm_valkind_enum};
use std::convert::{TryFrom, TryInto};
use std::ptr::NonNull;
use wasmer::Val;

#[allow(non_camel_case_types)]
pub type wasm_valkind_t = u8;

#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub union wasm_val_inner {
    pub(crate) int32_t: i32,
    pub(crate) int64_t: i64,
    pub(crate) float32_t: f32,
    pub(crate) float64_t: f64,
    pub(crate) wref: *mut wasm_ref_t,
}

#[allow(non_camel_case_types)]
#[repr(C)]
pub struct wasm_val_t {
    pub kind: wasm_valkind_t,
    pub of: wasm_val_inner,
}

wasm_declare_vec!(val);

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
