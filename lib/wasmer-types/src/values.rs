use crate::lib::std::fmt;
use crate::lib::std::ptr;
use crate::lib::std::string::{String, ToString};
use crate::r#ref::ExternRef;
use crate::types::Type;

/// Possible runtime values that a WebAssembly module can either consume or
/// produce.
#[derive(Clone, PartialEq)]
pub enum Value<T> {
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

    /// An `externref` value which can hold opaque data to the wasm instance itself.
    ///
    /// Note that this is a nullable value as well.
    ExternRef(ExternRef),

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

impl<T> Value<T> {
    /// Returns a null `externref` value.
    pub fn null() -> Self {
        Self::ExternRef(ExternRef::null())
    }

    /// Returns the corresponding [`Type`] for this `Value`.
    pub fn ty(&self) -> Type {
        match self {
            Self::I32(_) => Type::I32,
            Self::I64(_) => Type::I64,
            Self::F32(_) => Type::F32,
            Self::F64(_) => Type::F64,
            Self::ExternRef(_) => Type::ExternRef,
            Self::FuncRef(_) => Type::FuncRef,
            Self::V128(_) => Type::V128,
        }
    }

    /// Writes it's value to a given pointer
    ///
    /// # Safety
    /// `p` must be:
    /// - Sufficiently aligned for the Rust equivalent of the type in `self`
    /// - Non-null and pointing to valid, mutable memory
    pub unsafe fn write_value_to(&self, p: *mut i128) {
        match self {
            Self::I32(i) => ptr::write(p as *mut i32, *i),
            Self::I64(i) => ptr::write(p as *mut i64, *i),
            Self::F32(u) => ptr::write(p as *mut f32, *u),
            Self::F64(u) => ptr::write(p as *mut f64, *u),
            Self::V128(b) => ptr::write(p as *mut u128, *b),
            _ => unimplemented!("Value::write_value_to"),
        }
    }

    /// Gets a `Value` given a pointer and a `Type`
    ///
    /// # Safety
    /// `p` must be:
    /// - Properly aligned to the specified `ty`'s Rust equivalent
    /// - Non-null and pointing to valid memory
    pub unsafe fn read_value_from(p: *const i128, ty: Type) -> Self {
        match ty {
            Type::I32 => Self::I32(ptr::read(p as *const i32)),
            Type::I64 => Self::I64(ptr::read(p as *const i64)),
            Type::F32 => Self::F32(ptr::read(p as *const f32)),
            Type::F64 => Self::F64(ptr::read(p as *const f64)),
            Type::V128 => Self::V128(ptr::read(p as *const u128)),
            _ => unimplemented!("Value::read_value_from"),
        }
    }

    accessors! {
        e
        (I32(i32) i32 unwrap_i32 *e)
        (I64(i64) i64 unwrap_i64 *e)
        (F32(f32) f32 unwrap_f32 *e)
        (F64(f64) f64 unwrap_f64 *e)
        (FuncRef(&T) funcref unwrap_funcref e)
        (V128(u128) v128 unwrap_v128 *e)
    }

    /// Attempt to access the underlying value of this `Value`, returning
    /// `None` if it is not the correct type.
    ///
    /// This will return `Some` for both the `ExternRef` and `FuncRef` types.
    pub fn externref(&self) -> Option<ExternRef> {
        match self {
            Self::ExternRef(e) => Some(e.clone()),
            _ => None,
        }
    }

    /// Returns the underlying value of this `Value`, panicking if it's the
    /// wrong type.
    ///
    /// # Panics
    ///
    /// Panics if `self` is not of the right type.
    pub fn unwrap_externref(&self) -> ExternRef {
        self.externref().expect("expected externref")
    }
}

impl<T> fmt::Debug for Value<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::I32(v) => write!(f, "I32({:?})", v),
            Self::I64(v) => write!(f, "I64({:?})", v),
            Self::F32(v) => write!(f, "F32({:?})", v),
            Self::F64(v) => write!(f, "F64({:?})", v),
            Self::ExternRef(v) => write!(f, "ExternRef({:?})", v),
            Self::FuncRef(_) => write!(f, "FuncRef"),
            Self::V128(v) => write!(f, "V128({:?})", v),
        }
    }
}

impl<T> ToString for Value<T> {
    fn to_string(&self) -> String {
        match self {
            Self::I32(v) => v.to_string(),
            Self::I64(v) => v.to_string(),
            Self::F32(v) => v.to_string(),
            Self::F64(v) => v.to_string(),
            Self::ExternRef(_) => "externref".to_string(),
            Self::FuncRef(_) => "funcref".to_string(),
            Self::V128(v) => v.to_string(),
        }
    }
}

impl<T> From<i32> for Value<T> {
    fn from(val: i32) -> Self {
        Self::I32(val)
    }
}

impl<T> From<u32> for Value<T> {
    fn from(val: u32) -> Self {
        // In Wasm integers are sign-agnostic, so i32 is basically a 4 byte storage we can use for signed or unsigned 32-bit integers.
        Self::I32(val as i32)
    }
}

impl<T> From<i64> for Value<T> {
    fn from(val: i64) -> Self {
        Self::I64(val)
    }
}

impl<T> From<u64> for Value<T> {
    fn from(val: u64) -> Self {
        // In Wasm integers are sign-agnostic, so i64 is basically an 8 byte storage we can use for signed or unsigned 64-bit integers.
        Self::I64(val as i64)
    }
}

impl<T> From<f32> for Value<T> {
    fn from(val: f32) -> Self {
        Self::F32(val)
    }
}

impl<T> From<f64> for Value<T> {
    fn from(val: f64) -> Self {
        Self::F64(val)
    }
}

impl<T> From<ExternRef> for Value<T> {
    fn from(val: ExternRef) -> Self {
        Self::ExternRef(val)
    }
}

// impl<T> From<T> for Value<T> {
//     fn from(val: T) -> Self {
//         Self::FuncRef(val)
//     }
// }
