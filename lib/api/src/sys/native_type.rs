//! This module permits to create native functions
//! easily in Rust, thanks to its advanced typing system.

use wasmer_types::{NativeWasmType, RawValue, Type};
use wasmer_vm::{VMExternRef, VMFuncRef};

use crate::{ExternRef, Function, TypedFunction, WasmTypeList};

use super::store::AsStoreMut;

/// `NativeWasmTypeInto` performs conversions from and into `NativeWasmType`
/// types with a context.
pub trait NativeWasmTypeInto: NativeWasmType + Sized {
    #[doc(hidden)]
    fn into_abi(self, store: &mut impl AsStoreMut) -> Self::Abi;

    #[doc(hidden)]
    unsafe fn from_abi(store: &mut impl AsStoreMut, abi: Self::Abi) -> Self;

    /// Convert self to raw value representation.
    fn into_raw(self, store: &mut impl AsStoreMut) -> RawValue;

    /// Convert to self from raw value representation.
    ///
    /// # Safety
    ///
    unsafe fn from_raw(store: &mut impl AsStoreMut, raw: RawValue) -> Self;
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
    fn into_raw(self, _store: &mut impl AsStoreMut) -> RawValue {
        RawValue { i32: self }
    }

    #[inline]
    unsafe fn from_raw(_store: &mut impl AsStoreMut, raw: RawValue) -> Self {
        raw.i32
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
    fn into_raw(self, _store: &mut impl AsStoreMut) -> RawValue {
        RawValue { i64: self }
    }

    #[inline]
    unsafe fn from_raw(_store: &mut impl AsStoreMut, raw: RawValue) -> Self {
        raw.i64
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
    fn into_raw(self, _store: &mut impl AsStoreMut) -> RawValue {
        RawValue { f32: self }
    }

    #[inline]
    unsafe fn from_raw(_store: &mut impl AsStoreMut, raw: RawValue) -> Self {
        raw.f32
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
    fn into_raw(self, _store: &mut impl AsStoreMut) -> RawValue {
        RawValue { f64: self }
    }

    #[inline]
    unsafe fn from_raw(_store: &mut impl AsStoreMut, raw: RawValue) -> Self {
        raw.f64
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
    fn into_raw(self, _store: &mut impl AsStoreMut) -> RawValue {
        RawValue { u128: self }
    }

    #[inline]
    unsafe fn from_raw(_store: &mut impl AsStoreMut, raw: RawValue) -> Self {
        raw.u128
    }
}

impl NativeWasmType for ExternRef {
    const WASM_TYPE: Type = Type::ExternRef;
    type Abi = usize;
}

impl NativeWasmTypeInto for Option<ExternRef> {
    #[inline]
    unsafe fn from_abi(store: &mut impl AsStoreMut, abi: Self::Abi) -> Self {
        VMExternRef::from_raw(RawValue { externref: abi })
            .map(|e| ExternRef::from_vm_externref(store, e))
    }

    #[inline]
    fn into_abi(self, _store: &mut impl AsStoreMut) -> Self::Abi {
        self.map_or(0, |e| unsafe { e.vm_externref().into_raw().externref })
    }

    #[inline]
    fn into_raw(self, _store: &mut impl AsStoreMut) -> RawValue {
        self.map_or(RawValue { externref: 0 }, |e| e.vm_externref().into_raw())
    }

    #[inline]
    unsafe fn from_raw(store: &mut impl AsStoreMut, raw: RawValue) -> Self {
        VMExternRef::from_raw(raw).map(|e| ExternRef::from_vm_externref(store, e))
    }
}

impl<Args, Rets> From<TypedFunction<Args, Rets>> for Function
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    fn from(other: TypedFunction<Args, Rets>) -> Self {
        other.func
    }
}

impl NativeWasmType for Function {
    const WASM_TYPE: Type = Type::FuncRef;
    type Abi = usize;
}

#[cfg(feature = "compiler")]
impl NativeWasmTypeInto for Option<Function> {
    #[inline]
    unsafe fn from_abi(store: &mut impl AsStoreMut, abi: Self::Abi) -> Self {
        VMFuncRef::from_raw(RawValue { funcref: abi }).map(|f| Function::from_vm_funcref(store, f))
    }

    #[inline]
    fn into_abi(self, store: &mut impl AsStoreMut) -> Self::Abi {
        self.map_or(0, |f| unsafe { f.vm_funcref(store).into_raw().externref })
    }

    #[inline]
    fn into_raw(self, store: &mut impl AsStoreMut) -> RawValue {
        self.map_or(RawValue { externref: 0 }, |e| {
            e.vm_funcref(store).into_raw()
        })
    }

    #[inline]
    unsafe fn from_raw(store: &mut impl AsStoreMut, raw: RawValue) -> Self {
        VMFuncRef::from_raw(raw).map(|f| Function::from_vm_funcref(store, f))
    }
}

#[cfg(test)]
mod test_native_type {
    use super::*;
    use wasmer_types::Type;

    #[test]
    fn test_wasm_types() {
        assert_eq!(i32::WASM_TYPE, Type::I32);
        assert_eq!(i64::WASM_TYPE, Type::I64);
        assert_eq!(f32::WASM_TYPE, Type::F32);
        assert_eq!(f64::WASM_TYPE, Type::F64);
        assert_eq!(u128::WASM_TYPE, Type::V128);
    }
    /*
    #[test]
    fn test_roundtrip() {
        unsafe {
            assert_eq!(i32::from_raw(42i32.into_raw()), 42i32);
            assert_eq!(i64::from_raw(42i64.into_raw()), 42i64);
            assert_eq!(f32::from_raw(42f32.into_raw()), 42f32);
            assert_eq!(f64::from_raw(42f64.into_raw()), 42f64);
            assert_eq!(u128::from_raw(42u128.into_raw()), 42u128);
        }
    }
    */
}

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
