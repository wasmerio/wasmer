//! This module permits to create native functions
//! easily in Rust, thanks to it's advanced typing system.

use crate::types::{FunctionType, Type};
use std::convert::Infallible;
use std::marker::PhantomData;

/// `NativeWasmType` represents a native Wasm type.
/// It uses the Rust Type system to automatically detect the
/// Wasm type associated with a native Rust type.
///
/// ```
/// use wasm_common::{NativeWasmType, Type};
///
/// let wasm_type = i32::WASM_TYPE;
/// assert_eq!(wasm_type, Type::I32);
/// ```
///
/// > Note: This strategy will be needed later to
/// > automatically detect the signature of a Rust function.
pub trait NativeWasmType {
    /// The ABI for this type (i32, i64, f32, f64)
    type Abi: Copy + std::fmt::Debug;

    /// Type for this `NativeWasmType`.
    const WASM_TYPE: Type;

    #[doc(hidden)]
    fn from_abi(abi: Self::Abi) -> Self;

    #[doc(hidden)]
    fn into_abi(self) -> Self::Abi;

    /// Convert self to i128 binary representation.
    fn to_binary(self) -> i128;

    /// Convert to self from i128 binary representation.
    fn from_binary(binary: i128) -> Self;
}

impl NativeWasmType for i32 {
    const WASM_TYPE: Type = Type::I32;
    type Abi = Self;

    #[inline]
    fn from_abi(abi: Self::Abi) -> Self {
        abi
    }

    #[inline]
    fn into_abi(self) -> Self::Abi {
        self
    }

    fn to_binary(self) -> i128 {
        self as _
    }

    fn from_binary(bits: i128) -> Self {
        bits as _
    }
}
impl NativeWasmType for i64 {
    const WASM_TYPE: Type = Type::I64;
    type Abi = Self;

    #[inline]
    fn from_abi(abi: Self::Abi) -> Self {
        abi
    }

    #[inline]
    fn into_abi(self) -> Self::Abi {
        self
    }

    fn to_binary(self) -> i128 {
        self as _
    }

    fn from_binary(bits: i128) -> Self {
        bits as _
    }
}
impl NativeWasmType for f32 {
    const WASM_TYPE: Type = Type::F32;
    type Abi = Self;

    #[inline]
    fn from_abi(abi: Self::Abi) -> Self {
        abi
    }

    #[inline]
    fn into_abi(self) -> Self::Abi {
        self
    }

    fn to_binary(self) -> i128 {
        self.to_bits() as _
    }

    fn from_binary(bits: i128) -> Self {
        Self::from_bits(bits as _)
    }
}
impl NativeWasmType for f64 {
    const WASM_TYPE: Type = Type::F64;
    type Abi = Self;

    #[inline]
    fn from_abi(abi: Self::Abi) -> Self {
        abi
    }

    #[inline]
    fn into_abi(self) -> Self::Abi {
        self
    }

    fn to_binary(self) -> i128 {
        self.to_bits() as _
    }

    fn from_binary(bits: i128) -> Self {
        Self::from_bits(bits as _)
    }
}

#[cfg(test)]
mod test_native_type {
    use super::*;
    use crate::types::Type;

    #[test]
    fn test_wasm_types() {
        assert_eq!(i32::WASM_TYPE, Type::I32);
        assert_eq!(i64::WASM_TYPE, Type::I64);
        assert_eq!(f32::WASM_TYPE, Type::F32);
        assert_eq!(f64::WASM_TYPE, Type::F64);
    }
}

/// A trait to represent a wasm extern type.
pub unsafe trait WasmExternType: Copy
where
    Self: Sized,
{
    /// Native wasm type for this `WasmExternType`.
    type Native: NativeWasmType;

    /// Convert from given `Native` type to self.
    fn from_native(native: Self::Native) -> Self;

    /// Convert self to `Native` type.
    fn to_native(self) -> Self::Native;
}

macro_rules! wasm_extern_type {
    ($type:ty => $native_type:ty) => {
        unsafe impl WasmExternType for $type {
            type Native = $native_type;

            fn from_native(native: Self::Native) -> Self {
                native as _
            }

            fn to_native(self) -> Self::Native {
                self as _
            }
        }
    };
}

wasm_extern_type!(i8 => i32);
wasm_extern_type!(u8 => i32);
wasm_extern_type!(i16 => i32);
wasm_extern_type!(u16 => i32);
wasm_extern_type!(i32 => i32);
wasm_extern_type!(u32 => i32);
wasm_extern_type!(i64 => i64);
wasm_extern_type!(u64 => i64);
wasm_extern_type!(f32 => f32);
wasm_extern_type!(f64 => f64);
// wasm_extern_type!(u128 => i128);
// wasm_extern_type!(i128 => i128);

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

/// Trait for a Value type. A Value type is a type that is always valid and may
/// be safely copied.
///
/// That is, for all possible bit patterns a valid Value type can be constructed
/// from those bits.
///
/// Concretely a `u32` is a Value type because every combination of 32 bits is
/// a valid `u32`. However a `bool` is _not_ a Value type because any bit patterns
/// other than `0` and `1` are invalid in Rust and may cause undefined behavior if
/// a `bool` is constructed from those bytes.
pub unsafe trait ValueType: Copy
where
    Self: Sized,
{
}

macro_rules! convert_value_impl {
    ($t:ty) => {
        unsafe impl ValueType for $t {}
    };
    ( $($t:ty),* ) => {
        $(
            convert_value_impl!($t);
        )*
    };
}

convert_value_impl!(u8, i8, u16, i16, u32, i32, u64, i64, f32, f64);

/// Represents a list of WebAssembly values.
pub trait WasmTypeList {
    /// CStruct type.
    type CStruct;

    /// Array of return values.
    type Array: AsMut<[i128]>;

    /// Construct `Self` based on an array of returned values.
    fn from_array(array: Self::Array) -> Self;

    /// Transforms Rust values into an Array
    fn into_array(self) -> Self::Array;

    /// Generates an empty array that will hold the returned values of
    /// the WebAssembly function.
    fn empty_array() -> Self::Array;

    /// Transforms C values into Rust values.
    fn from_c_struct(c_struct: Self::CStruct) -> Self;

    /// Transforms Rust values into C values.
    fn into_c_struct(self) -> Self::CStruct;

    /// Get types of the current values.
    fn wasm_types() -> &'static [Type];
}

/// Represents a TrapEarly type.
pub trait TrapEarly<Rets>
where
    Rets: WasmTypeList,
{
    /// The error type for this trait.
    type Error: Send + 'static;
    /// Get returns or error result.
    fn report(self) -> Result<Rets, Self::Error>;
}

impl<Rets> TrapEarly<Rets> for Rets
where
    Rets: WasmTypeList,
{
    type Error = Infallible;
    fn report(self) -> Result<Rets, Infallible> {
        Ok(self)
    }
}

impl<Rets, E> TrapEarly<Rets> for Result<Rets, E>
where
    Rets: WasmTypeList,
    E: Send + 'static,
{
    type Error = E;
    fn report(self) -> Result<Rets, E> {
        self
    }
}

/// Empty trait to specify the kind of `HostFunction`: With or
/// without a `vm::Ctx` argument. See the `ExplicitVmCtx` and the
/// `ImplicitVmCtx` structures.
///
/// This trait is never aimed to be used by a user. It is used by the
/// trait system to automatically generate an appropriate `wrap`
/// function.
#[doc(hidden)]
pub trait HostFunctionKind {}

/// An empty struct to help Rust typing to determine
/// when a `HostFunction` doesn't take an Environment
pub struct WithEnv {}

impl HostFunctionKind for WithEnv {}

/// An empty struct to help Rust typing to determine
/// when a `HostFunction` takes an Environment
pub struct WithoutEnv {}

impl HostFunctionKind for WithoutEnv {}

/// Represents a function that can be converted to a `vm::Func`
/// (function pointer) that can be called within WebAssembly.
pub trait HostFunction<Args, Rets, Kind, T>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
    Kind: HostFunctionKind,
    T: Sized,
    Self: Sized,
{
    /// Convert to function pointer.
    fn to_raw(self) -> *const FunctionBody;
}

#[repr(transparent)]
pub struct FunctionBody(*mut u8);

/// Represents a function that can be used by WebAssembly.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Func<Args = (), Rets = ()> {
    address: *const FunctionBody,
    _phantom: PhantomData<(Args, Rets)>,
}

unsafe impl<Args, Rets> Send for Func<Args, Rets> {}

impl<Args, Rets> Func<Args, Rets>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    /// Creates a new `Func`.
    pub fn new<F, T, E>(func: F) -> Self
    where
        F: HostFunction<Args, Rets, T, E>,
        T: HostFunctionKind,
        E: Sized,
    {
        Self {
            address: func.to_raw(),
            _phantom: PhantomData,
        }
    }

    /// Get the type of the Func
    pub fn ty(&self) -> FunctionType {
        FunctionType::new(Args::wasm_types(), Rets::wasm_types())
    }

    /// Get the address of the Func
    pub fn address(&self) -> *const FunctionBody {
        self.address
    }
}

impl WasmTypeList for Infallible {
    type CStruct = Self;
    type Array = [i128; 0];

    fn from_array(_: Self::Array) -> Self {
        unreachable!()
    }

    fn into_array(self) -> Self::Array {
        []
    }

    fn empty_array() -> Self::Array {
        unreachable!()
    }

    fn from_c_struct(_: Self::CStruct) -> Self {
        unreachable!()
    }

    fn into_c_struct(self) -> Self::CStruct {
        unreachable!()
    }

    fn wasm_types() -> &'static [Type] {
        &[]
    }

    // #[allow(non_snake_case)]
    // unsafe fn call<Rets>(
    //     self,
    // ) -> Result<Rets>
    // where
    //     Rets: WasmTypeList,
    // {
    //     unreachable!()
    // }
}

macro_rules! impl_traits {
    ( [$repr:ident] $struct_name:ident, $( $x:ident ),* ) => {
        /// Struct for typed funcs.
        #[repr($repr)]
        pub struct $struct_name< $( $x ),* > ( $( <$x as WasmExternType>::Native ),* )
        where
            $( $x: WasmExternType ),*;

        #[allow(unused_parens, dead_code)]
        impl< $( $x ),* > WasmTypeList for ( $( $x ),* )
        where
            $( $x: WasmExternType ),*
        {
            type CStruct = $struct_name<$( $x ),*>;

            type Array = [i128; count_idents!( $( $x ),* )];

            fn from_array(array: Self::Array) -> Self {
                #[allow(non_snake_case)]
                let [ $( $x ),* ] = array;

                ( $( WasmExternType::from_native(NativeWasmType::from_binary($x)) ),* )
            }

            fn into_array(self) -> Self::Array {
                #[allow(non_snake_case)]
                let ( $( $x ),* ) = self;
                [ $( WasmExternType::to_native($x).to_binary() ),* ]
            }

            fn empty_array() -> Self::Array {
                [0; count_idents!( $( $x ),* )]
            }

            fn from_c_struct(c_struct: Self::CStruct) -> Self {
                #[allow(non_snake_case)]
                let $struct_name ( $( $x ),* ) = c_struct;

                ( $( WasmExternType::from_native($x) ),* )
            }

            #[allow(unused_parens, non_snake_case)]
            fn into_c_struct(self) -> Self::CStruct {
                let ( $( $x ),* ) = self;

                $struct_name ( $( WasmExternType::to_native($x) ),* )
            }

            fn wasm_types() -> &'static [Type] {
                &[$( $x::Native::WASM_TYPE ),*]
            }
        }

        #[allow(unused_parens)]
        impl< $( $x, )* Rets, FN > HostFunction<( $( $x ),* ), Rets, WithoutEnv, ()> for FN
        where
            $( $x: WasmExternType, )*
            Rets: WasmTypeList,
            FN: Fn($( $x , )*) -> Rets + 'static + Send
        {
            #[allow(non_snake_case)]
            fn to_raw(self) -> *const FunctionBody {
                // unimplemented!("");
                extern fn wrap<$( $x, )* Rets, FN>( _: usize, $($x: $x::Native, )* ) -> Rets::CStruct
                where
                    Rets: WasmTypeList,
                    $($x: WasmExternType,)*
                    FN: Fn( $( $x ),* ) -> Rets + 'static
                {
                    // println!("WRAP");
                    // println!("Struct {:?}", (($( $x ),*) as WasmTypeList).into_c_struct());
                    // $( println!("X: {:?}", $x); )*
                    let f: &FN = unsafe { std::mem::transmute(&()) };
                    f( $( WasmExternType::from_native($x) ),* ).into_c_struct()
                }
                wrap::<$( $x, )* Rets, Self> as *const FunctionBody

                // extern fn wrap<$( $x: WasmExternType, )* Rets>(a: &dyn Any, b: &dyn Any, $($x: $x, )* ) -> Rets::CStruct
                // where
                //     Rets: WasmTypeList
                // {
                //     println!("WRAP");
                //     let f: &fn( &dyn Any, &dyn Any, $( $x ),* ) -> Rets = unsafe { std::mem::transmute(&()) };
                //     f( a, b, $( $x ),* ).into_c_struct()
                // }
                // wrap::<$( $x, )* Rets> as *const FunctionBody

                // extern fn wrap<$( $x, )* Rets, FN>(
                //     $($x: <$x as WasmExternType>::Native , )*
                // ) -> Rets::CStruct
                // where
                //     $( $x: WasmExternType, )*
                //     Rets: WasmTypeList,
                //     FN: Fn($( $x, )*) -> Rets::CStruct,
                // {
                //     // let self_pointer = wrap::<$( $x, )* Rets, FN> as *const FunctionBody;
                //     let f: &FN = unsafe {
                //         std::mem::transmute(&())
                //     };
                //     f($( $x, )*)
                // }
                // unimplemented!("");
                // extern fn wrap<Args, Rets>(
                //     env: &FuncEnv,
                //     args: Args::Array,
                //     returns: Rets::Array
                // )
                // where
                //     Args: WasmTypeList,
                //     Rets: WasmTypeList,
                // {
                //     let self_pointer = wrap::<Args, Rets> as *const FunctionBody;
                //     self_pointer($( $x , )*);
                // }
                // unimplemented!("");
                //             $( $x: WasmExternType, )*
        //             Rets: WasmTypeList,
        //             Trap: TrapEarly<Rets>,
        //             FN: Fn($( $x, )*) -> Trap,
        //         {
        // let x = |args: <(i32, i32) as WasmTypeList>::Array, rets: &mut <(i32, i32) as WasmTypeList>::Array| {
        //     let result = func_i32_i32__i32_i32(args[0] as _, args[1] as _);
        //     rets[0] = result.0 as _;
        //     rets[1] = result.1 as _;
        // };

        //         &self as *const _ as *const FunctionBody
                // let x: *const FunctionBody = unsafe { std::mem::transmute(self) };
                // unimplemented!("");
            }
        }

        #[allow(unused_parens)]
        impl< $( $x, )* Rets, FN, T > HostFunction<( $( $x ),* ), Rets, WithEnv, T> for FN
        where
            $( $x: WasmExternType, )*
            Rets: WasmTypeList,
            T: Sized,
            FN: Fn(&mut T, $( $x , )*) -> Rets + 'static + Send
        {
            #[allow(non_snake_case)]
            fn to_raw(self) -> *const FunctionBody {
                extern fn wrap<$( $x, )* Rets, FN, T>( ctx: &mut T, $($x: $x::Native, )* ) -> Rets::CStruct
                where
                    Rets: WasmTypeList,
                    $($x: WasmExternType,)*
                    T: Sized,
                    FN: Fn(&mut T, $( $x ),* ) -> Rets + 'static
                {
                    let f: &FN = unsafe { std::mem::transmute(&()) };
                    f(ctx, $( WasmExternType::from_native($x) ),* ).into_c_struct()
                }
                wrap::<$( $x, )* Rets, Self, T> as *const FunctionBody
            }
        }
    };
}

macro_rules! count_idents {
    ( $($idents:ident),* ) => {{
        #[allow(dead_code, non_camel_case_types)]
        enum Idents { $($idents,)* __CountIdentsLast }
        const COUNT: usize = Idents::__CountIdentsLast as usize;
        COUNT
    }};
}

impl_traits!([C] S0,);
impl_traits!([transparent] S1, A1);
impl_traits!([C] S2, A1, A2);
impl_traits!([C] S3, A1, A2, A3);
impl_traits!([C] S4, A1, A2, A3, A4);
impl_traits!([C] S5, A1, A2, A3, A4, A5);
impl_traits!([C] S6, A1, A2, A3, A4, A5, A6);
impl_traits!([C] S7, A1, A2, A3, A4, A5, A6, A7);
impl_traits!([C] S8, A1, A2, A3, A4, A5, A6, A7, A8);
impl_traits!([C] S9, A1, A2, A3, A4, A5, A6, A7, A8, A9);
impl_traits!([C] S10, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10);
impl_traits!([C] S11, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11);
impl_traits!([C] S12, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12);
impl_traits!([C] S13, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13);
impl_traits!([C] S14, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14);
impl_traits!([C] S15, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15);
// impl_traits!([C] S16, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16);
// impl_traits!([C] S17, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17);
// impl_traits!([C] S18, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18);
// impl_traits!([C] S19, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19);
// impl_traits!([C] S20, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20);
// impl_traits!([C] S21, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21);
// impl_traits!([C] S22, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22);
// impl_traits!([C] S23, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23);
// impl_traits!([C] S24, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24);
// impl_traits!([C] S25, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24, A25);
// impl_traits!([C] S26, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24, A25, A26);

#[cfg(test)]
mod test_wasm_type_list {
    use super::*;
    use crate::types::Type;
    // WasmTypeList

    #[test]
    fn test_simple_values() {
        // Simple values
        assert_eq!(<i32>::wasm_types(), [Type::I32]);
        assert_eq!(<i64>::wasm_types(), [Type::I64]);
        assert_eq!(<f32>::wasm_types(), [Type::F32]);
        assert_eq!(<f64>::wasm_types(), [Type::F64]);

        // Multi values
        assert_eq!(<(i32, i32)>::wasm_types(), [Type::I32, Type::I32]);
        assert_eq!(<(i64, i64)>::wasm_types(), [Type::I64, Type::I64]);
        assert_eq!(<(f32, f32)>::wasm_types(), [Type::F32, Type::F32]);
        assert_eq!(<(f64, f64)>::wasm_types(), [Type::F64, Type::F64]);

        // Mixed values
        // assert_eq!(<(i32, i64, f32, f64)>::wasm_types(), [Type::I32, Type::I64, Type::F32, Type::F64]);
    }

    #[test]
    fn test_empty_array() {
        assert_eq!(<()>::empty_array().len(), 0);
        assert_eq!(<i32>::empty_array().len(), 1);
        assert_eq!(<(i32, i64)>::empty_array().len(), 2);
    }

    // #[test]
    // fn test_from_array() {
    //     assert_eq!(<()>::from_array([]), ());
    //     assert_eq!(<(i32)>::from_array([1]), (1));
    //     assert_eq!(<(i32, i32)>::from_array([1, 1]), (1, 1));
    //     // This doesn't work
    //     // assert_eq!(<(i32, i64, f32, f64)>::from_array([1, 2, (3.1f32).to_bits().into(), (4.2f64).to_bits().into()]), (1, 2, 3.1f32, 4.2f64));
    // }

    // #[test]
    // fn test_into_array() {
    //     assert_eq!(().into_array(), []);
    //     assert_eq!((1).into_array(), [1]);
    //     assert_eq!((1, 2).into_array(), [1, 2]);
    //     assert_eq!((1, 2, 3).into_array(), [1, 2, 3]);
    //     // This doesn't work
    //     // assert_eq!(<(i32, i64, f32, f64)>::from_array([1, 2, (3.1f32).to_bits().into(), (4.2f64).to_bits().into()]), (1, 2, 3.1f32, 4.2f64));
    // }

    #[test]
    fn test_into_c_struct() {
        // assert_eq!(<()>::into_c_struct(), &[]);
    }
}

#[allow(non_snake_case)]
#[cfg(test)]
mod test_func {
    use super::*;
    use crate::types::Type;
    // WasmTypeList

    fn func() {}
    fn func__i32() -> i32 {
        0
    }
    fn func_i32(_a: i32) {}
    fn func_i32__i32(a: i32) -> i32 {
        a * 2
    }
    fn func_i32_i32__i32(a: i32, b: i32) -> i32 {
        a + b
    }
    fn func_i32_i32__i32_i32(a: i32, b: i32) -> (i32, i32) {
        (a, b)
    }
    fn func_f32_i32__i32_f32(a: f32, b: i32) -> (i32, f32) {
        (b, a)
    }

    #[test]
    fn test_function_types() {
        assert_eq!(Function::new(func).ty(), FunctionType::new(vec![], vec![]));
        assert_eq!(
            Function::new(func__i32).ty(),
            FunctionType::new(vec![], vec![Type::I32])
        );
        assert_eq!(
            Function::new(func_i32).ty(),
            FunctionType::new(vec![Type::I32], vec![])
        );
        assert_eq!(
            Function::new(func_i32__i32).ty(),
            FunctionType::new(vec![Type::I32], vec![Type::I32])
        );
        assert_eq!(
            Function::new(func_i32_i32__i32).ty(),
            FunctionType::new(vec![Type::I32, Type::I32], vec![Type::I32])
        );
        assert_eq!(
            Function::new(func_i32_i32__i32_i32).ty(),
            FunctionType::new(vec![Type::I32, Type::I32], vec![Type::I32, Type::I32])
        );
        assert_eq!(
            Function::new(func_f32_i32__i32_f32).ty(),
            FunctionType::new(vec![Type::F32, Type::I32], vec![Type::I32, Type::F32])
        );
    }

    #[test]
    fn test_function_pointer() {
        let f = Function::new(func_i32__i32);
        let function = unsafe {
            std::mem::transmute::<*const FunctionBody, fn(i32, i32, i32) -> i32>(f.address)
        };
        assert_eq!(function(1, 2, 3), 6);
    }

    #[test]
    fn test_function_env_pointer() {
        fn func_i32__i32_env(env: &mut Env, a: i32) -> i32 {
            let result = env.num * a;
            env.num = 10;
            return result;
        }
        struct Env {
            pub num: i32,
        };
        let mut my_env = Env { num: 2 };
        let f = Function::new_env(&mut my_env, func_i32__i32_env);
        let function = unsafe {
            std::mem::transmute::<*const FunctionBody, fn(&mut Env, i32) -> i32>(f.address)
        };
        assert_eq!(function(&mut my_env, 3), 6);
        assert_eq!(my_env.num, 10);
    }

    #[test]
    fn test_function_call() {
        let f = Function::new(func_i32__i32);
        let x = |args: <(i32, i32) as WasmTypeList>::Array,
                 rets: &mut <(i32, i32) as WasmTypeList>::Array| {
            let result = func_i32_i32__i32_i32(args[0] as _, args[1] as _);
            rets[0] = result.0 as _;
            rets[1] = result.1 as _;
        };
        let mut rets = <(i64, i64)>::empty_array();
        x([20, 10], &mut rets);
        // panic!("Rets: {:?}",rets);
        let mut rets = <(i64)>::empty_array();
        // let result = f.call([1], &mut rets);
        // assert_eq!(result.is_err(), true);
    }
}
