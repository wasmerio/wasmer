//! This module permits to create native functions
//! easily in Rust, thanks to its advanced typing system.

use std::fmt;

use wasmer_types::{RawValue, Type};
use wasmer_vm::{VMExternRef, VMFuncRef};

use crate::{ExternRef, Function};

use super::context::AsContextMut;

/// `NativeWasmType` represents a Wasm type that has a direct
/// representation on the host (hence the “native” term).
///
/// It uses the Rust Type system to automatically detect the
/// Wasm type associated with a native Rust type.
///
/// ```
/// use wasmer_types::{NativeWasmType, Type};
///
/// let wasm_type = i32::WASM_TYPE;
/// assert_eq!(wasm_type, Type::I32);
/// ```
///
/// > Note: This strategy will be needed later to
/// > automatically detect the signature of a Rust function.
pub trait NativeWasmType: Sized {
    /// The ABI for this type (i32, i64, f32, f64)
    type Abi: Copy + fmt::Debug;

    /// Type for this `NativeWasmType`.
    const WASM_TYPE: Type;

    #[doc(hidden)]
    fn into_abi(self, ctx: &mut impl AsContextMut) -> Self::Abi;

    #[doc(hidden)]
    unsafe fn from_abi(ctx: &mut impl AsContextMut, abi: Self::Abi) -> Self;

    /// Convert self to raw value representation.
    fn into_raw(self, ctx: &mut impl AsContextMut) -> RawValue;

    /// Convert to self from raw value representation.
    unsafe fn from_raw(ctx: &mut impl AsContextMut, raw: RawValue) -> Self;
}

impl NativeWasmType for i32 {
    const WASM_TYPE: Type = Type::I32;
    type Abi = Self;

    #[inline]
    unsafe fn from_abi(_ctx: &mut impl AsContextMut, abi: Self::Abi) -> Self {
        abi
    }

    #[inline]
    fn into_abi(self, _ctx: &mut impl AsContextMut) -> Self::Abi {
        self
    }

    #[inline]
    fn into_raw(self, _ctx: &mut impl AsContextMut) -> RawValue {
        RawValue { i32: self }
    }

    #[inline]
    unsafe fn from_raw(_ctx: &mut impl AsContextMut, raw: RawValue) -> Self {
        raw.i32
    }
}

impl NativeWasmType for i64 {
    const WASM_TYPE: Type = Type::I64;
    type Abi = Self;

    #[inline]
    unsafe fn from_abi(_ctx: &mut impl AsContextMut, abi: Self::Abi) -> Self {
        abi
    }

    #[inline]
    fn into_abi(self, _ctx: &mut impl AsContextMut) -> Self::Abi {
        self
    }

    #[inline]
    fn into_raw(self, _ctx: &mut impl AsContextMut) -> RawValue {
        RawValue { i64: self }
    }

    #[inline]
    unsafe fn from_raw(_ctx: &mut impl AsContextMut, raw: RawValue) -> Self {
        raw.i64
    }
}

impl NativeWasmType for f32 {
    const WASM_TYPE: Type = Type::F32;
    type Abi = Self;

    #[inline]
    unsafe fn from_abi(_ctx: &mut impl AsContextMut, abi: Self::Abi) -> Self {
        abi
    }

    #[inline]
    fn into_abi(self, _ctx: &mut impl AsContextMut) -> Self::Abi {
        self
    }

    #[inline]
    fn into_raw(self, _ctx: &mut impl AsContextMut) -> RawValue {
        RawValue { f32: self }
    }

    #[inline]
    unsafe fn from_raw(_ctx: &mut impl AsContextMut, raw: RawValue) -> Self {
        raw.f32
    }
}

impl NativeWasmType for f64 {
    const WASM_TYPE: Type = Type::F64;
    type Abi = Self;

    #[inline]
    unsafe fn from_abi(_ctx: &mut impl AsContextMut, abi: Self::Abi) -> Self {
        abi
    }

    #[inline]
    fn into_abi(self, _ctx: &mut impl AsContextMut) -> Self::Abi {
        self
    }

    #[inline]
    fn into_raw(self, _ctx: &mut impl AsContextMut) -> RawValue {
        RawValue { f64: self }
    }

    #[inline]
    unsafe fn from_raw(_ctx: &mut impl AsContextMut, raw: RawValue) -> Self {
        raw.f64
    }
}

impl NativeWasmType for u128 {
    const WASM_TYPE: Type = Type::V128;
    type Abi = Self;

    #[inline]
    unsafe fn from_abi(_ctx: &mut impl AsContextMut, abi: Self::Abi) -> Self {
        abi
    }

    #[inline]
    fn into_abi(self, _ctx: &mut impl AsContextMut) -> Self::Abi {
        self
    }

    #[inline]
    fn into_raw(self, _ctx: &mut impl AsContextMut) -> RawValue {
        RawValue { u128: self }
    }

    #[inline]
    unsafe fn from_raw(_ctx: &mut impl AsContextMut, raw: RawValue) -> Self {
        raw.u128
    }
}

impl NativeWasmType for Option<ExternRef> {
    const WASM_TYPE: Type = Type::ExternRef;
    type Abi = usize;

    #[inline]
    unsafe fn from_abi(ctx: &mut impl AsContextMut, abi: Self::Abi) -> Self {
        VMExternRef::from_raw(RawValue { externref: abi })
            .map(|e| ExternRef::from_vm_externref(ctx, e))
    }

    #[inline]
    fn into_abi(self, _ctx: &mut impl AsContextMut) -> Self::Abi {
        self.map(|e| unsafe { e.vm_externref().into_raw().externref })
            .unwrap_or(0)
    }

    #[inline]
    fn into_raw(self, _ctx: &mut impl AsContextMut) -> RawValue {
        self.map(|e| e.vm_externref().into_raw())
            .unwrap_or(RawValue { externref: 0 })
    }

    #[inline]
    unsafe fn from_raw(ctx: &mut impl AsContextMut, raw: RawValue) -> Self {
        VMExternRef::from_raw(raw).map(|e| ExternRef::from_vm_externref(ctx, e))
    }
}

impl NativeWasmType for Option<Function> {
    const WASM_TYPE: Type = Type::FuncRef;
    type Abi = usize;

    #[inline]
    unsafe fn from_abi(ctx: &mut impl AsContextMut, abi: Self::Abi) -> Self {
        VMFuncRef::from_raw(RawValue { funcref: abi }).map(|f| Function::from_vm_funcref(ctx, f))
    }

    #[inline]
    fn into_abi(self, ctx: &mut impl AsContextMut) -> Self::Abi {
        self.map(|f| unsafe { f.vm_funcref(ctx).into_raw().externref })
            .unwrap_or(0)
    }

    #[inline]
    fn into_raw(self, ctx: &mut impl AsContextMut) -> RawValue {
        self.map(|e| e.vm_funcref(ctx).into_raw())
            .unwrap_or(RawValue { externref: 0 })
    }

    #[inline]
    unsafe fn from_raw(ctx: &mut impl AsContextMut, raw: RawValue) -> Self {
        VMFuncRef::from_raw(raw).map(|f| Function::from_vm_funcref(ctx, f))
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
