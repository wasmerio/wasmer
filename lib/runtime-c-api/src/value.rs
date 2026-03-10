//! Create and map Rust to WebAssembly values.

use wasmer_runtime::Value;
use wasmer_runtime_core::types::Type;

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

impl From<wasmer_value_t> for Value {
    fn from(v: wasmer_value_t) -> Self {
        unsafe {
            #[allow(unreachable_patterns, non_snake_case)]
            match v {
                wasmer_value_t {
                    tag: wasmer_value_tag::WASM_I32,
                    value: wasmer_value { I32 },
                } => Value::I32(I32),
                wasmer_value_t {
                    tag: wasmer_value_tag::WASM_I64,
                    value: wasmer_value { I64 },
                } => Value::I64(I64),
                wasmer_value_t {
                    tag: wasmer_value_tag::WASM_F32,
                    value: wasmer_value { F32 },
                } => Value::F32(F32),
                wasmer_value_t {
                    tag: wasmer_value_tag::WASM_F64,
                    value: wasmer_value { F64 },
                } => Value::F64(F64),
                _ => unreachable!("unknown WASM type"),
            }
        }
    }
}

impl From<Value> for wasmer_value_t {
    fn from(val: Value) -> Self {
        match val {
            Value::I32(x) => wasmer_value_t {
                tag: wasmer_value_tag::WASM_I32,
                value: wasmer_value { I32: x },
            },
            Value::I64(x) => wasmer_value_t {
                tag: wasmer_value_tag::WASM_I64,
                value: wasmer_value { I64: x },
            },
            Value::F32(x) => wasmer_value_t {
                tag: wasmer_value_tag::WASM_F32,
                value: wasmer_value { F32: x },
            },
            Value::F64(x) => wasmer_value_t {
                tag: wasmer_value_tag::WASM_F64,
                value: wasmer_value { F64: x },
            },
            Value::V128(_) => unimplemented!("V128 not supported in C API"),
        }
    }
}

impl From<Type> for wasmer_value_tag {
    fn from(ty: Type) -> Self {
        #[allow(unreachable_patterns)]
        match ty {
            Type::I32 => wasmer_value_tag::WASM_I32,
            Type::I64 => wasmer_value_tag::WASM_I64,
            Type::F32 => wasmer_value_tag::WASM_F32,
            Type::F64 => wasmer_value_tag::WASM_F64,
            Type::V128 => unreachable!("V128 not supported in C API"),
        }
    }
}

impl From<wasmer_value_tag> for Type {
    fn from(v: wasmer_value_tag) -> Self {
        #[allow(unreachable_patterns)]
        match v {
            wasmer_value_tag::WASM_I32 => Type::I32,
            wasmer_value_tag::WASM_I64 => Type::I64,
            wasmer_value_tag::WASM_F32 => Type::F32,
            wasmer_value_tag::WASM_F64 => Type::F64,
            _ => unreachable!("unknown WASM type"),
        }
    }
}

impl From<&wasmer_runtime::wasm::Type> for wasmer_value_tag {
    fn from(ty: &Type) -> Self {
        match *ty {
            Type::I32 => wasmer_value_tag::WASM_I32,
            Type::I64 => wasmer_value_tag::WASM_I64,
            Type::F32 => wasmer_value_tag::WASM_F32,
            Type::F64 => wasmer_value_tag::WASM_F64,
            Type::V128 => unimplemented!("V128 not supported in C API"),
        }
    }
}
