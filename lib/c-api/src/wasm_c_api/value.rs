use super::types::{wasm_ref_t, wasm_valkind_enum};
use std::convert::{TryFrom, TryInto};
use wasmer_api::Value;

/// Represents the kind of values. The variants of this C enum is
/// defined in `wasm.h` to list the following:
///
/// * `WASM_I32`, a 32-bit integer,
/// * `WASM_I64`, a 64-bit integer,
/// * `WASM_F32`, a 32-bit float,
/// * `WASM_F64`, a 64-bit float,
/// * `WASM_ANYREF`, a WebAssembly reference,
/// * `WASM_FUNCREF`, a WebAssembly reference.
#[allow(non_camel_case_types)]
pub type wasm_valkind_t = u8;

/// A Rust union, compatible with C, that holds a value of kind
/// [`wasm_valkind_t`] (see [`wasm_val_t`] to get the complete
/// picture). Members of the union are:
///
/// * `int32_t` if the value is a 32-bit integer,
/// * `int64_t` if the value is a 64-bit integer,
/// * `float32_t` if the value is a 32-bit float,
/// * `float64_t` if the value is a 64-bit float,
/// * `wref` (`wasm_ref_t`) if the value is a WebAssembly reference.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub union wasm_val_inner {
    pub(crate) int32_t: i32,
    pub(crate) int64_t: i64,
    pub(crate) float32_t: f32,
    pub(crate) float64_t: f64,
    pub(crate) wref: *mut wasm_ref_t,
}

/// A WebAssembly value composed of its type and its value.
///
/// Note that `wasm.h` defines macros to create Wasm values more
/// easily: `WASM_I32_VAL`, `WASM_I64_VAL`, `WASM_F32_VAL`,
/// `WASM_F64_VAL`, and `WASM_REF_VAL`.
///
/// # Example
///
/// ```rust
/// # use wasmer_inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer.h"
/// #
/// int main() {
///     // Create a 32-bit integer Wasm value.
///     wasm_val_t value1 = {
///         .kind = WASM_I32,
///         .of = { .i32 = 7 },
///     };
///
///     // Create the same value with the `wasm.h` macro.
///     wasm_val_t value2 = WASM_I32_VAL(7);
///
///     assert(value2.kind == WASM_I32);
///     assert(value1.of.i32 == value2.of.i32);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
#[allow(non_camel_case_types)]
#[repr(C)]
pub struct wasm_val_t {
    /// The kind of the value.
    pub kind: wasm_valkind_t,

    /// The real value.
    pub of: wasm_val_inner,
}

impl std::fmt::Debug for wasm_val_t {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut ds = f.debug_struct("wasm_val_t");
        ds.field("kind", &self.kind);

        match self.kind.try_into() {
            Ok(wasm_valkind_enum::WASM_I32) => {
                ds.field("i32", &unsafe { self.of.int32_t });
            }
            Ok(wasm_valkind_enum::WASM_I64) => {
                ds.field("i64", &unsafe { self.of.int64_t });
            }
            Ok(wasm_valkind_enum::WASM_F32) => {
                ds.field("f32", &unsafe { self.of.float32_t });
            }
            Ok(wasm_valkind_enum::WASM_F64) => {
                ds.field("f64", &unsafe { self.of.float64_t });
            }
            Ok(wasm_valkind_enum::WASM_ANYREF) => {
                ds.field("anyref", &unsafe { self.of.wref });
            }

            Ok(wasm_valkind_enum::WASM_FUNCREF) => {
                ds.field("funcref", &unsafe { self.of.wref });
            }
            Err(_) => {
                ds.field("value", &"Invalid value type");
            }
        }
        ds.finish()
    }
}

wasm_declare_vec!(val);

impl Clone for wasm_val_t {
    fn clone(&self) -> Self {
        wasm_val_t {
            kind: self.kind,
            of: self.of,
        }
    }
}

impl Default for wasm_val_t {
    fn default() -> Self {
        Self {
            kind: wasm_valkind_enum::WASM_I64 as _,
            of: wasm_val_inner { int64_t: 0 },
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_val_copy(
    // own
    out: &mut wasm_val_t,
    val: &wasm_val_t,
) {
    out.kind = val.kind;
    out.of = c_try!(val.kind.try_into().map(|kind| {
        match kind {
            wasm_valkind_enum::WASM_I32 => wasm_val_inner {
                int32_t: val.of.int32_t,
            },
            wasm_valkind_enum::WASM_I64 => wasm_val_inner {
                int64_t: val.of.int64_t,
            },
            wasm_valkind_enum::WASM_F32 => wasm_val_inner {
                float32_t: val.of.float32_t,
            },
            wasm_valkind_enum::WASM_F64 => wasm_val_inner {
                float64_t: val.of.float64_t,
            },
            wasm_valkind_enum::WASM_ANYREF => wasm_val_inner { wref: val.of.wref },
            wasm_valkind_enum::WASM_FUNCREF => wasm_val_inner { wref: val.of.wref },
        }
    }); otherwise ());
}

#[no_mangle]
pub unsafe extern "C" fn wasm_val_delete(val: Option<Box<wasm_val_t>>) {
    if let Some(val) = val {
        // TODO: figure out where wasm_val is allocated first...
        let _ = val;
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

impl TryFrom<wasm_val_t> for Value {
    type Error = &'static str;

    fn try_from(item: wasm_val_t) -> Result<Self, Self::Error> {
        (&item).try_into()
    }
}

impl TryFrom<&wasm_val_t> for Value {
    type Error = &'static str;

    fn try_from(item: &wasm_val_t) -> Result<Self, Self::Error> {
        Ok(match item.kind.try_into()? {
            wasm_valkind_enum::WASM_I32 => Value::I32(unsafe { item.of.int32_t }),
            wasm_valkind_enum::WASM_I64 => Value::I64(unsafe { item.of.int64_t }),
            wasm_valkind_enum::WASM_F32 => Value::F32(unsafe { item.of.float32_t }),
            wasm_valkind_enum::WASM_F64 => Value::F64(unsafe { item.of.float64_t }),
            wasm_valkind_enum::WASM_ANYREF => return Err("ANYREF not supported at this time"),
            wasm_valkind_enum::WASM_FUNCREF => return Err("FUNCREF not supported at this time"),
        })
    }
}

impl TryFrom<Value> for wasm_val_t {
    type Error = &'static str;

    fn try_from(item: Value) -> Result<Self, Self::Error> {
        wasm_val_t::try_from(&item)
    }
}

impl TryFrom<&Value> for wasm_val_t {
    type Error = &'static str;

    fn try_from(item: &Value) -> Result<Self, Self::Error> {
        Ok(match *item {
            Value::I32(v) => wasm_val_t {
                of: wasm_val_inner { int32_t: v },
                kind: wasm_valkind_enum::WASM_I32 as _,
            },
            Value::I64(v) => wasm_val_t {
                of: wasm_val_inner { int64_t: v },
                kind: wasm_valkind_enum::WASM_I64 as _,
            },
            Value::F32(v) => wasm_val_t {
                of: wasm_val_inner { float32_t: v },
                kind: wasm_valkind_enum::WASM_F32 as _,
            },
            Value::F64(v) => wasm_val_t {
                of: wasm_val_inner { float64_t: v },
                kind: wasm_valkind_enum::WASM_F64 as _,
            },
            Value::V128(_) => return Err("128bit SIMD types not yet supported in Wasm C API"),
            _ => todo!("Handle these values in TryFrom<Value> for wasm_val_t"),
        })
    }
}
