use crate::r#ref::AnyRef;
use crate::types::Type;
// use crate::native::Func;
use std::fmt;
use std::ptr;

/// Possible runtime values that a WebAssembly module can either consume or
/// produce.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Value<T> {
    /// A 32-bit integer
    I32(i32),

    /// A 64-bit integer
    I64(i64),

    /// A 32-bit float.
    ///
    /// Note that the raw bits of the float are stored here, and you can use
    /// `f32::from_bits` to create an `f32` value.
    F32(u32),

    /// A 64-bit float.
    ///
    /// Note that the raw bits of the float are stored here, and you can use
    /// `f64::from_bits` to create an `f64` value.
    F64(u64),

    /// An `anyref` value which can hold opaque data to the wasm instance itself.
    ///
    /// Note that this is a nullable value as well.
    AnyRef(AnyRef),

    /// A first-class reference to a WebAssembly function.
    FuncRef(T),

    /// A 128-bit number
    V128(u128),
}

macro_rules! accessors {
    ($bind:ident $(($variant:ident($ty:ty) $get:ident $unwrap:ident $cvt:expr))*) => ($(
        /// Attempt to access the underlying value of this `Value`, returning
        /// `None` if it is not the correct type.
        pub fn $get(&self) -> Option<$ty> {
            if let Value::$variant($bind) = self {
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

impl<T> Value<T> {
    /// Returns a null `anyref` value.
    pub fn null() -> Value<T> {
        Value::AnyRef(AnyRef::null())
    }

    /// Returns the corresponding [`Type`] for this `Value`.
    pub fn ty(&self) -> Type {
        match self {
            Value::I32(_) => Type::I32,
            Value::I64(_) => Type::I64,
            Value::F32(_) => Type::F32,
            Value::F64(_) => Type::F64,
            Value::AnyRef(_) => Type::AnyRef,
            Value::FuncRef(_) => Type::FuncRef,
            Value::V128(_) => Type::V128,
        }
    }

    /// Writes it's value to a given pointer
    pub unsafe fn write_value_to(&self, p: *mut i128) {
        match self {
            Value::I32(i) => ptr::write(p as *mut i32, *i),
            Value::I64(i) => ptr::write(p as *mut i64, *i),
            Value::F32(u) => ptr::write(p as *mut u32, *u),
            Value::F64(u) => ptr::write(p as *mut u64, *u),
            Value::V128(b) => ptr::write(p as *mut u128, *b),
            _ => unimplemented!("Value::write_value_to"),
        }
    }

    /// Get's a `Value` given a pointer and a `Type`
    pub unsafe fn read_value_from(p: *const i128, ty: Type) -> Value<T> {
        match ty {
            Type::I32 => Value::I32(ptr::read(p as *const i32)),
            Type::I64 => Value::I64(ptr::read(p as *const i64)),
            Type::F32 => Value::F32(ptr::read(p as *const u32)),
            Type::F64 => Value::F64(ptr::read(p as *const u64)),
            Type::V128 => Value::V128(ptr::read(p as *const u128)),
            _ => unimplemented!("Value::read_value_from"),
        }
    }

    accessors! {
        e
        (I32(i32) i32 unwrap_i32 *e)
        (I64(i64) i64 unwrap_i64 *e)
        (F32(f32) f32 unwrap_f32 f32::from_bits(*e))
        (F64(f64) f64 unwrap_f64 f64::from_bits(*e))
        (FuncRef(&T) funcref unwrap_funcref e)
        (V128(u128) v128 unwrap_v128 *e)
    }

    /// Attempt to access the underlying value of this `Value`, returning
    /// `None` if it is not the correct type.
    ///
    /// This will return `Some` for both the `AnyRef` and `FuncRef` types.
    pub fn anyref(&self) -> Option<AnyRef> {
        match self {
            Value::AnyRef(e) => Some(e.clone()),
            _ => None,
        }
    }

    /// Returns the underlying value of this `Value`, panicking if it's the
    /// wrong type.
    ///
    /// # Panics
    ///
    /// Panics if `self` is not of the right type.
    pub fn unwrap_anyref(&self) -> AnyRef {
        self.anyref().expect("expected anyref")
    }
}

impl<T> fmt::Debug for Value<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::I32(v) => write!(f, "i32:{:?}", v),
            Value::I64(v) => write!(f, "i64:{:?}", v),
            Value::F32(v) => write!(f, "f32:{:?}", v),
            Value::F64(v) => write!(f, "f64:{:?}", v),
            Value::AnyRef(v) => write!(f, "anyref:{:?}", v),
            Value::FuncRef(_) => write!(f, "funcref"),
            Value::V128(v) => write!(f, "v128:{:?}", v),
        }
    }
}

impl<T> From<i32> for Value<T> {
    fn from(val: i32) -> Value<T> {
        Value::I32(val)
    }
}

impl<T> From<i64> for Value<T> {
    fn from(val: i64) -> Value<T> {
        Value::I64(val)
    }
}

impl<T> From<f32> for Value<T> {
    fn from(val: f32) -> Value<T> {
        Value::F32(val.to_bits())
    }
}

impl<T> From<f64> for Value<T> {
    fn from(val: f64) -> Value<T> {
        Value::F64(val.to_bits())
    }
}

impl<T> From<AnyRef> for Value<T> {
    fn from(val: AnyRef) -> Value<T> {
        Value::AnyRef(val)
    }
}

// impl<T> From<T> for Value<T> {
//     fn from(val: T) -> Value<T> {
//         Value::FuncRef(val)
//     }
// }
