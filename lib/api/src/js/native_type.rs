//! This module permits to create native functions
//! easily in Rust, thanks to its advanced typing system.

use wasmer_types::{NativeWasmType, Type};

use crate::js::Function;

use super::store::AsStoreMut;

/// `NativeWasmTypeInto` performs conversions from and into `NativeWasmType`
/// types with a context.
pub trait NativeWasmTypeInto: NativeWasmType + Sized {
    #[doc(hidden)]
    fn into_abi(self, store: &mut impl AsStoreMut) -> Self::Abi;

    #[doc(hidden)]
    unsafe fn from_abi(store: &mut impl AsStoreMut, abi: Self::Abi) -> Self;

    /// Convert self to raw value representation.
    fn into_raw(self, store: &mut impl AsStoreMut) -> f64;

    /// Convert to self from raw value representation.
    ///
    /// # Safety
    ///
    unsafe fn from_raw(store: &mut impl AsStoreMut, raw: f64) -> Self;
}

impl NativeWasmTypeInto for i32 {
    #[inline]
    unsafe fn from_abi(_store: &mut impl AsStoreMut, abi: Self::Abi) -> Self {
        abi
    }

    #[inline]
    fn into_abi(self, _store: &mut impl AsStoreMut) -> Self::Abi {
        self
    }

    #[inline]
    fn into_raw(self, _store: &mut impl AsStoreMut) -> f64 {
        self.into()
    }

    #[inline]
    unsafe fn from_raw(_store: &mut impl AsStoreMut, raw: f64) -> Self {
        raw as _
    }
}

impl NativeWasmTypeInto for i64 {
    #[inline]
    unsafe fn from_abi(_store: &mut impl AsStoreMut, abi: Self::Abi) -> Self {
        abi
    }

    #[inline]
    fn into_abi(self, _store: &mut impl AsStoreMut) -> Self::Abi {
        self
    }

    #[inline]
    fn into_raw(self, _store: &mut impl AsStoreMut) -> f64 {
        self as _
    }

    #[inline]
    unsafe fn from_raw(_store: &mut impl AsStoreMut, raw: f64) -> Self {
        raw as _
    }
}

impl NativeWasmTypeInto for f32 {
    #[inline]
    unsafe fn from_abi(_store: &mut impl AsStoreMut, abi: Self::Abi) -> Self {
        abi
    }

    #[inline]
    fn into_abi(self, _store: &mut impl AsStoreMut) -> Self::Abi {
        self
    }

    #[inline]
    fn into_raw(self, _store: &mut impl AsStoreMut) -> f64 {
        self as _
    }

    #[inline]
    unsafe fn from_raw(_store: &mut impl AsStoreMut, raw: f64) -> Self {
        raw as _
    }
}

impl NativeWasmTypeInto for f64 {
    #[inline]
    unsafe fn from_abi(_store: &mut impl AsStoreMut, abi: Self::Abi) -> Self {
        abi
    }

    #[inline]
    fn into_abi(self, _store: &mut impl AsStoreMut) -> Self::Abi {
        self
    }

    #[inline]
    fn into_raw(self, _store: &mut impl AsStoreMut) -> f64 {
        self
    }

    #[inline]
    unsafe fn from_raw(_store: &mut impl AsStoreMut, raw: f64) -> Self {
        raw
    }
}

impl NativeWasmTypeInto for u128 {
    #[inline]
    unsafe fn from_abi(_store: &mut impl AsStoreMut, abi: Self::Abi) -> Self {
        abi
    }

    #[inline]
    fn into_abi(self, _store: &mut impl AsStoreMut) -> Self::Abi {
        self
    }

    #[inline]
    fn into_raw(self, _store: &mut impl AsStoreMut) -> f64 {
        self as _
    }

    #[inline]
    unsafe fn from_raw(_store: &mut impl AsStoreMut, raw: f64) -> Self {
        raw as _
    }
}

impl NativeWasmType for Function {
    const WASM_TYPE: Type = Type::FuncRef;
    type Abi = f64;
}

/*
mod test_native_type {
    use super::*;
    use wasmer_types::Type;

    fn test_wasm_types() {
        assert_eq!(i32::WASM_TYPE, Type::I32);
        assert_eq!(i64::WASM_TYPE, Type::I64);
        assert_eq!(f32::WASM_TYPE, Type::F32);
        assert_eq!(f64::WASM_TYPE, Type::F64);
    }

    fn test_roundtrip() {
        unsafe {
            assert_eq!(i32::from_raw(42i32.into_raw()), 42i32);
            assert_eq!(i64::from_raw(42i64.into_raw()), 42i64);
            assert_eq!(f32::from_raw(42f32.into_raw()), 42f32);
            assert_eq!(f64::from_raw(42f64.into_raw()), 42f64);
        }
    }
}
    */

// pub trait IntegerAtomic
// where
//     Self: Sized
// {
//     type Primitive;

//     fn add(&self, other: Self::Primitive) -> Self::Primitive;
//     fn sub(&self, other: Self::Primitive) -> Self::Primitive;
//     fn and(&self, other: Self::Primitive) -> Self::Primitive;
//     fn or(&self, other: Self::Primitive) -> Self::Primitive;
//     fn xor(&self, other: Self::Primitive) -> Self::Primitive;
//     fn load(&self) -> Self::Primitive;
//     fn store(&self, other: Self::Primitive) -> Self::Primitive;
//     fn compare_exchange(&self, expected: Self::Primitive, new: Self::Primitive) -> Self::Primitive;
//     fn swap(&self, other: Self::Primitive) -> Self::Primitive;
// }
