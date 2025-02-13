use wasmer_types::{RawValue, Type};

use crate::{
    entities::{ExceptionRef, ExternRef, Function},
    vm::{VMExceptionRef, VMExternRef, VMFuncRef},
    AsStoreRef, Tag,
};

/// WebAssembly computations manipulate values of basic value types:
/// * Integers (32 or 64 bit width)
/// * Floating-point (32 or 64 bit width)
/// * Vectors (128 bits, with 32 or 64 bit lanes)
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#values>
#[derive(Clone)]
pub enum Value {
    /// A 32-bit integer.
    ///
    /// In Wasm integers are sign-agnostic, i.e. this can either be signed or unsigned.
    I32(i32),

    /// A 64-bit integer.
    ///
    /// In Wasm integers are sign-agnostic, i.e. this can either be signed or unsigned.
    I64(i64),

    /// A 32-bit float.
    F32(f32),

    /// A 64-bit float.
    F64(f64),

    /// A 128-bit number
    V128(u128),

    // -- references --
    /// A nullable `externref` value which can hold opaque data to the wasm instance itself.
    ExternRef(Option<ExternRef>),

    /// A nullable first-class reference to a WebAssembly function.
    FuncRef(Option<Function>),

    /// A nullable first-class reference to a WebAssembly exception.
    ExceptionRef(Option<ExceptionRef>),
}

macro_rules! accessors {
    ($bind:ident $(($variant:ident($ty:ty) $get:ident $unwrap:ident $cvt:expr))*) => ($(
        /// Attempt to access the underlying value of this `Value`, returning
        /// `None` if it is not the correct type.
        pub fn $get(&self) -> Option<$ty> {
            if let Self::$variant($bind) = self {
                Some($cvt)
            } else {
                None
            }
        }

        /// Returns the underlying value of this `Value`, panicking if it's the
        /// wrong type.
        ///
        /// # Panics
        ///
        /// Panics if `self` is not of the right type.
        pub fn $unwrap(&self) -> $ty {
            self.$get().expect(concat!("expected ", stringify!($ty)))
        }
    )*)
}

impl Value {
    /// Returns a null `externref` value.
    pub fn null() -> Self {
        Self::ExternRef(None)
    }

    /// Returns the corresponding [`Type`] for this [`Value`].
    pub fn ty(&self) -> Type {
        match self {
            Self::I32(_) => Type::I32,
            Self::I64(_) => Type::I64,
            Self::F32(_) => Type::F32,
            Self::F64(_) => Type::F64,
            Self::V128(_) => Type::V128,
            Self::ExternRef(_) => Type::ExternRef,
            Self::FuncRef(_) => Type::FuncRef,
            Self::ExceptionRef(_) => Type::ExceptionRef,
        }
    }

    /// Converts the `Value` into a `RawValue`.
    pub fn as_raw(&self, store: &impl AsStoreRef) -> RawValue {
        match *self {
            Self::I32(i32) => RawValue { i32 },
            Self::I64(i64) => RawValue { i64 },
            Self::F32(f32) => RawValue { f32 },
            Self::F64(f64) => RawValue { f64 },
            Self::V128(u128) => RawValue { u128 },
            Self::ExceptionRef(Some(ref f)) => f.vm_exceptionref().into_raw(),
            Self::ExceptionRef(None) => RawValue { funcref: 0 },
            Self::FuncRef(Some(ref f)) => f.vm_funcref(store).into_raw(),
            Self::FuncRef(None) => RawValue { funcref: 0 },
            Self::ExternRef(Some(ref e)) => e.vm_externref().into_raw(),
            Self::ExternRef(None) => RawValue { externref: 0 },
        }
    }

    /// Converts a `RawValue` to a `Value`.
    ///
    /// # Safety
    ///
    pub unsafe fn from_raw(
        store: &mut impl crate::entities::store::AsStoreMut,
        ty: Type,
        raw: RawValue,
    ) -> Self {
        match ty {
            Type::I32 => Self::I32(raw.i32),
            Type::I64 => Self::I64(raw.i64),
            Type::F32 => Self::F32(raw.f32),
            Type::F64 => Self::F64(raw.f64),
            Type::V128 => Self::V128(raw.u128),
            Type::FuncRef => match store.as_store_ref().inner.store {
                #[cfg(feature = "sys")]
                crate::BackendStore::Sys(_) => Self::FuncRef(
                    crate::backend::sys::vm::VMFuncRef::from_raw(raw)
                        .map(VMFuncRef::Sys)
                        .map(|f| Function::from_vm_funcref(store, f)),
                ),
                #[cfg(feature = "wamr")]
                crate::BackendStore::Wamr(_) => Self::FuncRef(
                    crate::backend::wamr::vm::VMFuncRef::from_raw(raw)
                        .map(VMFuncRef::Wamr)
                        .map(|f| Function::from_vm_funcref(store, f)),
                ),
                #[cfg(feature = "wasmi")]
                crate::BackendStore::Wasmi(_) => Self::FuncRef(
                    crate::backend::wasmi::vm::VMFuncRef::from_raw(raw)
                        .map(VMFuncRef::Wasmi)
                        .map(|f| Function::from_vm_funcref(store, f)),
                ),

                #[cfg(feature = "v8")]
                crate::BackendStore::V8(_) => Self::FuncRef(
                    crate::backend::v8::vm::VMFuncRef::from_raw(raw)
                        .map(VMFuncRef::V8)
                        .map(|f| Function::from_vm_funcref(store, f)),
                ),
                #[cfg(feature = "js")]
                crate::BackendStore::Js(_) => Self::FuncRef(
                    crate::backend::js::vm::VMFuncRef::from_raw(raw)
                        .map(VMFuncRef::Js)
                        .map(|f| Function::from_vm_funcref(store, f)),
                ),
                #[cfg(feature = "jsc")]
                crate::BackendStore::Jsc(_) => Self::FuncRef(
                    crate::backend::jsc::vm::VMFuncRef::from_raw(raw)
                        .map(VMFuncRef::Jsc)
                        .map(|f| Function::from_vm_funcref(store, f)),
                ),
            },
            Type::ExternRef => match store.as_store_ref().inner.store {
                #[cfg(feature = "sys")]
                crate::BackendStore::Sys(_) => Self::ExternRef(
                    crate::backend::sys::vm::VMExternRef::from_raw(raw)
                        .map(VMExternRef::Sys)
                        .map(|f| ExternRef::from_vm_externref(store, f)),
                ),
                #[cfg(feature = "wamr")]
                crate::BackendStore::Wamr(_) => Self::ExternRef(
                    crate::backend::wamr::vm::VMExternRef::from_raw(raw)
                        .map(VMExternRef::Wamr)
                        .map(|f| ExternRef::from_vm_externref(store, f)),
                ),
                #[cfg(feature = "wasmi")]
                crate::BackendStore::Wasmi(_) => Self::ExternRef(
                    crate::backend::wasmi::vm::VMExternRef::from_raw(raw)
                        .map(VMExternRef::Wasmi)
                        .map(|f| ExternRef::from_vm_externref(store, f)),
                ),

                #[cfg(feature = "v8")]
                crate::BackendStore::V8(_) => Self::ExternRef(
                    crate::backend::v8::vm::VMExternRef::from_raw(raw)
                        .map(VMExternRef::V8)
                        .map(|f| ExternRef::from_vm_externref(store, f)),
                ),
                #[cfg(feature = "js")]
                crate::BackendStore::Js(_) => Self::ExternRef(
                    crate::backend::js::vm::VMExternRef::from_raw(raw)
                        .map(VMExternRef::Js)
                        .map(|f| ExternRef::from_vm_externref(store, f)),
                ),
                #[cfg(feature = "jsc")]
                crate::BackendStore::Jsc(_) => Self::ExternRef(
                    crate::backend::jsc::vm::VMExternRef::from_raw(raw)
                        .map(VMExternRef::Jsc)
                        .map(|f| ExternRef::from_vm_externref(store, f)),
                ),
            },
            Type::ExceptionRef => match store.as_store_ref().inner.store {
                #[cfg(feature = "sys")]
                crate::BackendStore::Sys(_) => Self::ExceptionRef(
                    crate::backend::sys::vm::VMExceptionRef::from_raw(raw)
                        .map(VMExceptionRef::Sys)
                        .map(|f| ExceptionRef::from_vm_exceptionref(store, f)),
                ),
                #[cfg(feature = "wamr")]
                crate::BackendStore::Wamr(_) => Self::ExceptionRef(
                    crate::backend::wamr::vm::VMExceptionRef::from_raw(raw)
                        .map(VMExceptionRef::Wamr)
                        .map(|f| ExceptionRef::from_vm_exceptionref(store, f)),
                ),
                #[cfg(feature = "wasmi")]
                crate::BackendStore::Wasmi(_) => Self::ExceptionRef(
                    crate::backend::wasmi::vm::VMExceptionRef::from_raw(raw)
                        .map(VMExceptionRef::Wasmi)
                        .map(|f| ExceptionRef::from_vm_exceptionref(store, f)),
                ),

                #[cfg(feature = "v8")]
                crate::BackendStore::V8(_) => Self::ExceptionRef(
                    crate::backend::v8::vm::VMExceptionRef::from_raw(raw)
                        .map(VMExceptionRef::V8)
                        .map(|f| ExceptionRef::from_vm_exceptionref(store, f)),
                ),
                #[cfg(feature = "js")]
                crate::BackendStore::Js(_) => Self::ExceptionRef(
                    crate::backend::js::vm::VMExceptionRef::from_raw(raw)
                        .map(VMExceptionRef::Js)
                        .map(|f| ExceptionRef::from_vm_exceptionref(store, f)),
                ),
                #[cfg(feature = "jsc")]
                crate::BackendStore::Jsc(_) => Self::ExceptionRef(
                    crate::backend::jsc::vm::VMExceptionRef::from_raw(raw)
                        .map(VMExceptionRef::Jsc)
                        .map(|f| ExceptionRef::from_vm_exceptionref(store, f)),
                ),
            },
        }
    }

    /// Checks whether a value can be used with the given context.
    ///
    /// Primitive (`i32`, `i64`, etc) and null funcref/externref values are not
    /// tied to a context and can be freely shared between contexts.
    ///
    /// Externref and funcref values are tied to a context and can only be used
    /// with that context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        match self {
            Self::I32(_)
            | Self::I64(_)
            | Self::F32(_)
            | Self::F64(_)
            | Self::V128(_)
            | Self::ExternRef(None)
            | Self::ExceptionRef(None)
            | Self::FuncRef(None) => true,
            Self::ExternRef(Some(e)) => e.is_from_store(store),
            Self::ExceptionRef(Some(e)) => e.is_from_store(store),
            Self::FuncRef(Some(f)) => f.is_from_store(store),
        }
    }

    accessors! {
        e
        (I32(i32) i32 unwrap_i32 *e)
        (I64(i64) i64 unwrap_i64 *e)
        (F32(f32) f32 unwrap_f32 *e)
        (F64(f64) f64 unwrap_f64 *e)
        (ExternRef(&Option<ExternRef>) externref unwrap_externref e)
        (FuncRef(&Option<Function>) funcref unwrap_funcref e)
        (V128(u128) v128 unwrap_v128 *e)
    }
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::I32(v) => write!(f, "I32({v:?})"),
            Self::I64(v) => write!(f, "I64({v:?})"),
            Self::F32(v) => write!(f, "F32({v:?})"),
            Self::F64(v) => write!(f, "F64({v:?})"),
            Self::ExceptionRef(None) => write!(f, "Null ExceptionRef"),
            Self::ExceptionRef(Some(v)) => write!(f, "ExceptionRef({v:?})"),
            Self::ExternRef(None) => write!(f, "Null ExternRef"),
            Self::ExternRef(Some(v)) => write!(f, "ExternRef({v:?})"),
            Self::FuncRef(None) => write!(f, "Null FuncRef"),
            Self::FuncRef(Some(v)) => write!(f, "FuncRef({v:?})"),
            Self::V128(v) => write!(f, "V128({v:?})"),
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::I32(v) => v.to_string(),
                Self::I64(v) => v.to_string(),
                Self::F32(v) => v.to_string(),
                Self::F64(v) => v.to_string(),
                Self::ExceptionRef(_) => "exnref".to_string(),
                Self::ExternRef(_) => "externref".to_string(),
                Self::FuncRef(_) => "funcref".to_string(),
                Self::V128(v) => v.to_string(),
            }
        )
    }
}

impl PartialEq for Value {
    fn eq(&self, o: &Self) -> bool {
        match (self, o) {
            (Self::I32(a), Self::I32(b)) => a == b,
            (Self::I64(a), Self::I64(b)) => a == b,
            (Self::F32(a), Self::F32(b)) => a == b,
            (Self::F64(a), Self::F64(b)) => a == b,
            (Self::V128(a), Self::V128(b)) => a == b,
            _ => false,
        }
    }
}

impl From<i32> for Value {
    fn from(val: i32) -> Self {
        Self::I32(val)
    }
}

impl From<u32> for Value {
    fn from(val: u32) -> Self {
        // In Wasm integers are sign-agnostic, so i32 is basically a 4 byte storage we can use for signed or unsigned 32-bit integers.
        Self::I32(val as i32)
    }
}

impl From<i64> for Value {
    fn from(val: i64) -> Self {
        Self::I64(val)
    }
}

impl From<u64> for Value {
    fn from(val: u64) -> Self {
        // In Wasm integers are sign-agnostic, so i64 is basically an 8 byte storage we can use for signed or unsigned 64-bit integers.
        Self::I64(val as i64)
    }
}

impl From<f32> for Value {
    fn from(val: f32) -> Self {
        Self::F32(val)
    }
}

impl From<f64> for Value {
    fn from(val: f64) -> Self {
        Self::F64(val)
    }
}

impl From<Function> for Value {
    fn from(val: Function) -> Self {
        Self::FuncRef(Some(val))
    }
}

impl From<Option<Function>> for Value {
    fn from(val: Option<Function>) -> Self {
        Self::FuncRef(val)
    }
}

impl From<ExternRef> for Value {
    fn from(val: ExternRef) -> Self {
        Self::ExternRef(Some(val))
    }
}

impl From<Option<ExternRef>> for Value {
    fn from(val: Option<ExternRef>) -> Self {
        Self::ExternRef(val)
    }
}

impl From<ExceptionRef> for Value {
    fn from(val: ExceptionRef) -> Self {
        Self::ExceptionRef(Some(val))
    }
}

impl From<Option<ExceptionRef>> for Value {
    fn from(val: Option<ExceptionRef>) -> Self {
        Self::ExceptionRef(val)
    }
}

const NOT_I32: &str = "Value is not of Wasm type i32";
const NOT_I64: &str = "Value is not of Wasm type i64";
const NOT_F32: &str = "Value is not of Wasm type f32";
const NOT_F64: &str = "Value is not of Wasm type f64";
const NOT_FUNCREF: &str = "Value is not of Wasm type funcref";
const NOT_EXTERNREF: &str = "Value is not of Wasm type externref";
const NOT_EXCEPTIONREF: &str = "Value is not of Wasm type exceptionref";

impl TryFrom<Value> for i32 {
    type Error = &'static str;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        value.i32().ok_or(NOT_I32)
    }
}

impl TryFrom<Value> for u32 {
    type Error = &'static str;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        value.i32().ok_or(NOT_I32).map(|int| int as Self)
    }
}

impl TryFrom<Value> for i64 {
    type Error = &'static str;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        value.i64().ok_or(NOT_I64)
    }
}

impl TryFrom<Value> for u64 {
    type Error = &'static str;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        value.i64().ok_or(NOT_I64).map(|int| int as Self)
    }
}

impl TryFrom<Value> for f32 {
    type Error = &'static str;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        value.f32().ok_or(NOT_F32)
    }
}

impl TryFrom<Value> for f64 {
    type Error = &'static str;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        value.f64().ok_or(NOT_F64)
    }
}

impl TryFrom<Value> for Option<Function> {
    type Error = &'static str;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::FuncRef(f) => Ok(f),
            _ => Err(NOT_FUNCREF),
        }
    }
}

impl TryFrom<Value> for Option<ExternRef> {
    type Error = &'static str;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::ExternRef(e) => Ok(e),
            _ => Err(NOT_EXTERNREF),
        }
    }
}

impl TryFrom<Value> for Option<ExceptionRef> {
    type Error = &'static str;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::ExceptionRef(e) => Ok(e),
            _ => Err(NOT_EXCEPTIONREF),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_i32_from_u32() {
        let bytes = [0x00, 0x00, 0x00, 0x00];
        let v = Value::from(u32::from_be_bytes(bytes));
        assert_eq!(v, Value::I32(i32::from_be_bytes(bytes)));

        let bytes = [0x00, 0x00, 0x00, 0x01];
        let v = Value::from(u32::from_be_bytes(bytes));
        assert_eq!(v, Value::I32(i32::from_be_bytes(bytes)));

        let bytes = [0xAA, 0xBB, 0xCC, 0xDD];
        let v = Value::from(u32::from_be_bytes(bytes));
        assert_eq!(v, Value::I32(i32::from_be_bytes(bytes)));

        let bytes = [0xFF, 0xFF, 0xFF, 0xFF];
        let v = Value::from(u32::from_be_bytes(bytes));
        assert_eq!(v, Value::I32(i32::from_be_bytes(bytes)));
    }

    #[test]
    fn test_value_i64_from_u64() {
        let bytes = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let v = Value::from(u64::from_be_bytes(bytes));
        assert_eq!(v, Value::I64(i64::from_be_bytes(bytes)));

        let bytes = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
        let v = Value::from(u64::from_be_bytes(bytes));
        assert_eq!(v, Value::I64(i64::from_be_bytes(bytes)));

        let bytes = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x11];
        let v = Value::from(u64::from_be_bytes(bytes));
        assert_eq!(v, Value::I64(i64::from_be_bytes(bytes)));

        let bytes = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let v = Value::from(u64::from_be_bytes(bytes));
        assert_eq!(v, Value::I64(i64::from_be_bytes(bytes)));
    }

    #[test]
    fn convert_value_to_i32() {
        let value = Value::I32(5678);
        let result = i32::try_from(value);
        assert_eq!(result.unwrap(), 5678);

        let value = Value::from(u32::MAX);
        let result = i32::try_from(value);
        assert_eq!(result.unwrap(), -1);

        let value = Value::V128(42);
        let result = i32::try_from(value);
        assert_eq!(result.unwrap_err(), "Value is not of Wasm type i32");
    }

    #[test]
    fn convert_value_to_u32() {
        let value = Value::from(u32::MAX);
        let result = u32::try_from(value);
        assert_eq!(result.unwrap(), u32::MAX);

        let value = Value::I32(-1);
        let result = u32::try_from(value);
        assert_eq!(result.unwrap(), u32::MAX);

        let value = Value::V128(42);
        let result = u32::try_from(value);
        assert_eq!(result.unwrap_err(), "Value is not of Wasm type i32");
    }

    #[test]
    fn convert_value_to_i64() {
        let value = Value::I64(5678);
        let result = i64::try_from(value);
        assert_eq!(result.unwrap(), 5678);

        let value = Value::from(u64::MAX);
        let result = i64::try_from(value);
        assert_eq!(result.unwrap(), -1);

        let value = Value::V128(42);
        let result = i64::try_from(value);
        assert_eq!(result.unwrap_err(), "Value is not of Wasm type i64");
    }

    #[test]
    fn convert_value_to_u64() {
        let value = Value::from(u64::MAX);
        let result = u64::try_from(value);
        assert_eq!(result.unwrap(), u64::MAX);

        let value = Value::I64(-1);
        let result = u64::try_from(value);
        assert_eq!(result.unwrap(), u64::MAX);

        let value = Value::V128(42);
        let result = u64::try_from(value);
        assert_eq!(result.unwrap_err(), "Value is not of Wasm type i64");
    }

    #[test]
    fn convert_value_to_f32() {
        let value = Value::F32(1.234);
        let result = f32::try_from(value);
        assert_eq!(result.unwrap(), 1.234);

        let value = Value::V128(42);
        let result = f32::try_from(value);
        assert_eq!(result.unwrap_err(), "Value is not of Wasm type f32");

        let value = Value::F64(1.234);
        let result = f32::try_from(value);
        assert_eq!(result.unwrap_err(), "Value is not of Wasm type f32");
    }

    #[test]
    fn convert_value_to_f64() {
        let value = Value::F64(1.234);
        let result = f64::try_from(value);
        assert_eq!(result.unwrap(), 1.234);

        let value = Value::V128(42);
        let result = f64::try_from(value);
        assert_eq!(result.unwrap_err(), "Value is not of Wasm type f64");

        let value = Value::F32(1.234);
        let result = f64::try_from(value);
        assert_eq!(result.unwrap_err(), "Value is not of Wasm type f64");
    }
}
