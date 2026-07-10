use super::store::StoreRef;
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
/// * `WASM_EXTERNREF`, a WebAssembly reference,
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
/// # use inline_c::assert_c;
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
            Ok(wasm_valkind_enum::WASM_EXTERNREF) => {
                ds.field("anyref", &unsafe { self.of.wref });
            }
            Ok(wasm_valkind_enum::WASM_FUNCREF) => {
                ds.field("funcref", &unsafe { self.of.wref });
            }
            Ok(wasm_valkind_enum::WASM_EXNREF) => {
                ds.field("exnref", &unsafe { self.of.wref });
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
        // Reference values own their boxed `wasm_ref_t`, so a shallow copy of
        // the pointer would double-free on drop. Deep-copy the box instead.
        // Kept in sync with `Drop`, which only frees EXTERNREF/FUNCREF (EXNREF
        // is never boxed, so it stays a plain, non-owning copy).
        match self.kind.try_into() {
            Ok(wasm_valkind_enum::WASM_EXTERNREF) | Ok(wasm_valkind_enum::WASM_FUNCREF) => {
                let wref = unsafe { self.of.wref };
                let cloned = if wref.is_null() {
                    std::ptr::null_mut()
                } else {
                    Box::into_raw(Box::new(unsafe { &*wref }.clone()))
                };
                wasm_val_t {
                    kind: self.kind,
                    of: wasm_val_inner { wref: cloned },
                }
            }
            _ => wasm_val_t {
                kind: self.kind,
                of: self.of,
            },
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_val_copy(
    // own
    out: &mut wasm_val_t,
    val: &wasm_val_t,
) {
    // `out` is an owned (uninitialized) out-parameter, so write into it without
    // running `Drop` on its prior (stale) contents. `Clone` deep-copies refs.
    unsafe { std::ptr::write(out, val.clone()) };
}

impl Drop for wasm_val_t {
    fn drop(&mut self) {
        let kind: Result<wasm_valkind_enum, _> = self.kind.try_into();
        match kind {
            Ok(wasm_valkind_enum::WASM_EXTERNREF) | Ok(wasm_valkind_enum::WASM_FUNCREF) => unsafe {
                if !self.of.wref.is_null() {
                    drop(Box::from_raw(self.of.wref));
                }
            },
            _ => {}
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_val_delete(val: *mut wasm_val_t) {
    if !val.is_null() {
        unsafe {
            std::ptr::drop_in_place(val);
        }
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
            128 => wasm_valkind_enum::WASM_EXTERNREF,
            129 => wasm_valkind_enum::WASM_FUNCREF,
            130 => wasm_valkind_enum::WASM_EXNREF,
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
            wasm_valkind_enum::WASM_EXTERNREF => {
                let wref = unsafe { item.of.wref };
                if wref.is_null() {
                    Value::ExternRef(None)
                } else {
                    // The boxed `wasm_ref_t` carries the authoritative value.
                    unsafe { &*wref }.inner.clone()
                }
            }
            wasm_valkind_enum::WASM_FUNCREF => {
                let wref = unsafe { item.of.wref };
                if wref.is_null() {
                    Value::FuncRef(None)
                } else {
                    unsafe { &*wref }.inner.clone()
                }
            }
            wasm_valkind_enum::WASM_EXNREF => return Err("EXNREF not supported at this time"),
        })
    }
}

impl wasm_val_t {
    /// Convert a [`Value`] into a [`wasm_val_t`], boxing reference values into a
    /// [`wasm_ref_t`] rooted in `store`. Null references become a null pointer.
    pub(crate) fn from_value(value: &Value, store: &StoreRef) -> Result<wasm_val_t, &'static str> {
        Ok(match value {
            Value::ExternRef(None) => wasm_val_t {
                kind: wasm_valkind_enum::WASM_EXTERNREF as _,
                of: wasm_val_inner {
                    wref: std::ptr::null_mut(),
                },
            },
            Value::FuncRef(None) => wasm_val_t {
                kind: wasm_valkind_enum::WASM_FUNCREF as _,
                of: wasm_val_inner {
                    wref: std::ptr::null_mut(),
                },
            },
            Value::ExternRef(Some(_)) | Value::FuncRef(Some(_)) => {
                let kind = if matches!(value, Value::ExternRef(_)) {
                    wasm_valkind_enum::WASM_EXTERNREF
                } else {
                    wasm_valkind_enum::WASM_FUNCREF
                };
                // `wasm_ref_t::new` returns `Some` for the `Some(_)` variants.
                let boxed = wasm_ref_t::new(store.clone(), value.clone())
                    .ok_or("failed to box reference value")?;
                wasm_val_t {
                    kind: kind as _,
                    of: wasm_val_inner {
                        wref: Box::into_raw(boxed),
                    },
                }
            }
            other => wasm_val_t::try_from(other)?,
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
            // Reference values need a store to box into a `wasm_ref_t`; callers
            // must use `wasm_val_t::from_value` instead.
            Value::ExternRef(_) | Value::FuncRef(_) | Value::ExceptionRef(_) => {
                return Err("reference values require a store; use wasm_val_t::from_value");
            }
        })
    }
}
