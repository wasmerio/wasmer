//! Create and map Rust to WebAssembly values.

use wasmer::Val;
use wasmer::ValType;

/// Represents all possibles WebAssembly value types.
///
/// See `wasmer_value_t` to get a complete example.
#[allow(non_camel_case_types)]
#[repr(u32)]
#[derive(Clone)]
pub enum wasmer_value_tag {
    /// Represents the `i32` WebAssembly type.
    WASM_I32,

    /// Represents the `i64` WebAssembly type.
    WASM_I64,

    /// Represents the `f32` WebAssembly type.
    WASM_F32,

    /// Represents the `f64` WebAssembly type.
    WASM_F64,
}

/// Represents a WebAssembly value.
///
/// This is a [Rust union][rust-union], which is equivalent to the C
/// union. See `wasmer_value_t` to get a complete example.
///
/// [rust-union]: https://doc.rust-lang.org/reference/items/unions.html
#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_snake_case)]
pub union wasmer_value {
    pub I32: i32,
    pub I64: i64,
    pub F32: f32,
    pub F64: f64,
}

/// Represents a WebAssembly type and value pair,
/// i.e. `wasmer_value_tag` and `wasmer_value`. Since the latter is an
/// union, it's the safe way to read or write a WebAssembly value in
/// C.
///
/// Example:
///
/// ```c
/// // Create a WebAssembly value.
/// wasmer_value_t wasm_value = {
///     .tag = WASM_I32,
///     .value.I32 = 42,
/// };
///
/// // Read a WebAssembly value.
/// if (wasm_value.tag == WASM_I32) {
///     int32_t x = wasm_value.value.I32;
///     // …
/// }
/// ```
#[repr(C)]
#[derive(Clone)]
pub struct wasmer_value_t {
    /// The value type.
    pub tag: wasmer_value_tag,

    /// The value.
    pub value: wasmer_value,
}

impl From<wasmer_value_t> for Val {
    fn from(v: wasmer_value_t) -> Self {
        unsafe {
            #[allow(unreachable_patterns, non_snake_case)]
            match v {
                wasmer_value_t {
                    tag: wasmer_value_tag::WASM_I32,
                    value: wasmer_value { I32 },
                } => Val::I32(I32),
                wasmer_value_t {
                    tag: wasmer_value_tag::WASM_I64,
                    value: wasmer_value { I64 },
                } => Val::I64(I64),
                wasmer_value_t {
                    tag: wasmer_value_tag::WASM_F32,
                    value: wasmer_value { F32 },
                } => Val::F32(F32),
                wasmer_value_t {
                    tag: wasmer_value_tag::WASM_F64,
                    value: wasmer_value { F64 },
                } => Val::F64(F64),
                _ => unreachable!("unknown WASM type"),
            }
        }
    }
}

impl From<Val> for wasmer_value_t {
    fn from(val: Val) -> Self {
        match val {
            Val::I32(x) => wasmer_value_t {
                tag: wasmer_value_tag::WASM_I32,
                value: wasmer_value { I32: x },
            },
            Val::I64(x) => wasmer_value_t {
                tag: wasmer_value_tag::WASM_I64,
                value: wasmer_value { I64: x },
            },
            Val::F32(x) => wasmer_value_t {
                tag: wasmer_value_tag::WASM_F32,
                value: wasmer_value { F32: x },
            },
            Val::F64(x) => wasmer_value_t {
                tag: wasmer_value_tag::WASM_F64,
                value: wasmer_value { F64: x },
            },
            Val::V128(_) => unimplemented!("V128 not supported in C API"),
            Val::AnyRef(_) => unimplemented!("AnyRef not supported in C API"),
            Val::FuncRef(_) => unimplemented!("AnyFunc not supported in C API"),
        }
    }
}

impl From<ValType> for wasmer_value_tag {
    fn from(ty: ValType) -> Self {
        #[allow(unreachable_patterns)]
        match ty {
            ValType::I32 => wasmer_value_tag::WASM_I32,
            ValType::I64 => wasmer_value_tag::WASM_I64,
            ValType::F32 => wasmer_value_tag::WASM_F32,
            ValType::F64 => wasmer_value_tag::WASM_F64,
            ValType::V128 => unreachable!("V128 not supported in C API"),
            ValType::AnyRef => unimplemented!("AnyRef not supported in C API"),
            ValType::FuncRef => unimplemented!("FuncRef not supported in C API"),
        }
    }
}

impl From<wasmer_value_tag> for ValType {
    fn from(v: wasmer_value_tag) -> Self {
        #[allow(unreachable_patterns)]
        match v {
            wasmer_value_tag::WASM_I32 => ValType::I32,
            wasmer_value_tag::WASM_I64 => ValType::I64,
            wasmer_value_tag::WASM_F32 => ValType::F32,
            wasmer_value_tag::WASM_F64 => ValType::F64,
            _ => unreachable!("unknown WASM type"),
        }
    }
}

impl From<&ValType> for wasmer_value_tag {
    fn from(ty: &ValType) -> Self {
        match *ty {
            ValType::I32 => wasmer_value_tag::WASM_I32,
            ValType::I64 => wasmer_value_tag::WASM_I64,
            ValType::F32 => wasmer_value_tag::WASM_F32,
            ValType::F64 => wasmer_value_tag::WASM_F64,
            ValType::V128 => unimplemented!("V128 not supported in C API"),
            ValType::AnyRef => unimplemented!("AnyRef not supported in C API"),
            ValType::FuncRef => unimplemented!("FuncRef not supported in C API"),
        }
    }
}
