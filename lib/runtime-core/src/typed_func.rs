use crate::{
    error::RuntimeError,
    export::{Context, Export, FuncPointer},
    import::IsExport,
    types::{FuncSig, NativeWasmType, Type, WasmExternType},
    vm,
};
use std::{
    any::Any, convert::Infallible, ffi::c_void, fmt, marker::PhantomData, mem, panic, ptr::NonNull,
    sync::Arc,
};

#[repr(C)]
pub enum WasmTrapInfo {
    Unreachable = 0,
    IncorrectCallIndirectSignature = 1,
    MemoryOutOfBounds = 2,
    CallIndirectOOB = 3,
    IllegalArithmetic = 4,
    MisalignedAtomicAccess = 5,
    Unknown,
}

impl fmt::Display for WasmTrapInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                WasmTrapInfo::Unreachable => "unreachable",
                WasmTrapInfo::IncorrectCallIndirectSignature => {
                    "incorrect `call_indirect` signature"
                }
                WasmTrapInfo::MemoryOutOfBounds => "memory out-of-bounds access",
                WasmTrapInfo::CallIndirectOOB => "`call_indirect` out-of-bounds",
                WasmTrapInfo::IllegalArithmetic => "illegal arithmetic operation",
                WasmTrapInfo::MisalignedAtomicAccess => "misaligned atomic access",
                WasmTrapInfo::Unknown => "unknown",
            }
        )
    }
}

/// This is just an empty trait to constrict that types that
/// can be put into the third/fourth (depending if you include lifetimes)
/// of the `Func` struct.
pub trait Kind {}

pub type Trampoline = unsafe extern "C" fn(
    func_env: Option<NonNull<vm::FuncEnv>>,
    func: NonNull<vm::Func>,
    args: *const u64,
    rets: *mut u64,
);

pub type Invoke = unsafe extern "C" fn(
    trampoline: Trampoline,
    vmctx: Option<NonNull<vm::Ctx>>,
    func_env: Option<NonNull<vm::FuncEnv>>,
    func: NonNull<vm::Func>,
    args: *const u64,
    rets: *mut u64,
    trap_info: *mut WasmTrapInfo,
    user_error: *mut Option<Box<dyn Any>>,
    extra: Option<NonNull<c_void>>,
) -> bool;

/// TODO(lachlan): Naming TBD.
/// This contains the trampoline and invoke functions for a specific signature,
/// as well as the environment that the invoke function may or may not require.
#[derive(Copy, Clone)]
pub struct Wasm {
    pub(crate) trampoline: Trampoline,
    pub(crate) invoke: Invoke,
    pub(crate) invoke_env: Option<NonNull<c_void>>,
}

impl Wasm {
    pub unsafe fn from_raw_parts(
        trampoline: Trampoline,
        invoke: Invoke,
        invoke_env: Option<NonNull<c_void>>,
    ) -> Self {
        Self {
            trampoline,
            invoke,
            invoke_env,
        }
    }
}

/// This type, as part of the `Func` type signature, represents a function that is created
/// by the host.
pub struct Host(());

impl Kind for Wasm {}
impl Kind for Host {}

/// Represents a list of WebAssembly values.
pub trait WasmTypeList {
    type CStruct;

    type RetArray: AsMut<[u64]>;

    /// Construct `Self` based on an array of returned values.
    fn from_ret_array(array: Self::RetArray) -> Self;

    /// Generates an empty array that will hold the returned values of
    /// the WebAssembly function.
    fn empty_ret_array() -> Self::RetArray;

    /// Transforms C values into Rust values.
    fn from_c_struct(c_struct: Self::CStruct) -> Self;

    /// Transforms Rust values into C values.
    fn into_c_struct(self) -> Self::CStruct;

    /// Get types of the current values.
    fn types() -> &'static [Type];

    /// This method is used to distribute the values onto a function,
    /// e.g. `(1, 2).call(func, …)`. This form is unlikely to be used
    /// directly in the code, see the `Func:call` implementation.
    unsafe fn call<Rets>(
        self,
        func: NonNull<vm::Func>,
        func_env: Option<NonNull<vm::FuncEnv>>,
        vmctx: Option<NonNull<vm::Ctx>>,
        wasm: Wasm,
    ) -> Result<Rets, RuntimeError>
    where
        Rets: WasmTypeList;
}

pub trait ExternalFunctionKind {}

pub struct ExplicitVmCtx();
pub struct ClosedEnvironment();

impl ExternalFunctionKind for ExplicitVmCtx {}
impl ExternalFunctionKind for ClosedEnvironment {}

/// Represents a function that can be converted to a `vm::Func`
/// (function pointer) that can be called within WebAssembly.
pub trait ExternalFunction<Kind, Args, Rets>
where
    Kind: ExternalFunctionKind,
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    fn to_raw(self) -> (NonNull<vm::Func>, Option<NonNull<vm::FuncEnv>>);
}

pub trait TrapEarly<Rets>
where
    Rets: WasmTypeList,
{
    type Error: 'static;
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
    E: 'static,
{
    type Error = E;
    fn report(self) -> Result<Rets, E> {
        self
    }
}

/// Represents a function that can be used by WebAssembly.
pub struct Func<'a, Args = (), Rets = (), Inner: Kind = Wasm> {
    inner: Inner,
    func: NonNull<vm::Func>,
    func_env: Option<NonNull<vm::FuncEnv>>,
    vmctx: Option<NonNull<vm::Ctx>>,
    _phantom: PhantomData<(&'a (), Args, Rets)>,
}

unsafe impl<'a, Args, Rets> Send for Func<'a, Args, Rets, Wasm> {}
unsafe impl<'a, Args, Rets> Send for Func<'a, Args, Rets, Host> {}

impl<'a, Args, Rets> Func<'a, Args, Rets, Wasm>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    pub(crate) unsafe fn from_raw_parts(
        inner: Wasm,
        func: NonNull<vm::Func>,
        func_env: Option<NonNull<vm::FuncEnv>>,
        vmctx: Option<NonNull<vm::Ctx>>,
    ) -> Func<'a, Args, Rets, Wasm> {
        Func {
            inner,
            func,
            func_env,
            vmctx,
            _phantom: PhantomData,
        }
    }

    pub fn get_vm_func(&self) -> NonNull<vm::Func> {
        self.func
    }
}

impl<'a, Args, Rets> Func<'a, Args, Rets, Host>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    pub fn new<F, Kind>(func: F) -> Func<'a, Args, Rets, Host>
    where
        Kind: ExternalFunctionKind,
        F: ExternalFunction<Kind, Args, Rets>,
    {
        let (func, func_env) = func.to_raw();

        Func {
            inner: Host(()),
            func,
            func_env,
            vmctx: None,
            _phantom: PhantomData,
        }
    }
}

impl<'a, Args, Rets, Inner> Func<'a, Args, Rets, Inner>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
    Inner: Kind,
{
    /// Returns the types of the function inputs.
    pub fn params(&self) -> &'static [Type] {
        Args::types()
    }

    /// Returns the types of the function outputs.
    pub fn returns(&self) -> &'static [Type] {
        Rets::types()
    }
}

impl WasmTypeList for Infallible {
    type CStruct = Infallible;
    type RetArray = [u64; 0];

    fn from_ret_array(_: Self::RetArray) -> Self {
        unreachable!()
    }

    fn empty_ret_array() -> Self::RetArray {
        unreachable!()
    }

    fn from_c_struct(_: Self::CStruct) -> Self {
        unreachable!()
    }

    fn into_c_struct(self) -> Self::CStruct {
        unreachable!()
    }

    fn types() -> &'static [Type] {
        &[]
    }

    #[allow(non_snake_case)]
    unsafe fn call<Rets>(
        self,
        _: NonNull<vm::Func>,
        _: Option<NonNull<vm::FuncEnv>>,
        _: Option<NonNull<vm::Ctx>>,
        _: Wasm,
    ) -> Result<Rets, RuntimeError>
    where
        Rets: WasmTypeList,
    {
        unreachable!()
    }
}

macro_rules! impl_traits {
    ( [$repr:ident] $struct_name:ident, $( $x:ident ),* ) => {
        #[repr($repr)]
        pub struct $struct_name< $( $x ),* > ( $( <$x as WasmExternType>::Native ),* )
        where
            $( $x: WasmExternType ),*;

        impl< $( $x ),* > WasmTypeList for ( $( $x ),* )
        where
            $( $x: WasmExternType ),*
        {
            type CStruct = $struct_name<$( $x ),*>;

            type RetArray = [u64; count_idents!( $( $x ),* )];

            fn from_ret_array(array: Self::RetArray) -> Self {
                #[allow(non_snake_case)]
                let [ $( $x ),* ] = array;

                ( $( WasmExternType::from_native(NativeWasmType::from_binary($x)) ),* )
            }

            fn empty_ret_array() -> Self::RetArray {
                [0; count_idents!( $( $x ),* )]
            }

            fn from_c_struct(c_struct: Self::CStruct) -> Self {
                #[allow(non_snake_case)]
                let $struct_name ( $( $x ),* ) = c_struct;

                ( $( WasmExternType::from_native($x) ),* )
            }

            fn into_c_struct(self) -> Self::CStruct {
                #[allow(unused_parens, non_snake_case)]
                let ( $( $x ),* ) = self;

                $struct_name ( $( WasmExternType::to_native($x) ),* )
            }

            fn types() -> &'static [Type] {
                &[$( $x::Native::TYPE ),*]
            }

            #[allow(non_snake_case)]
            unsafe fn call<Rets>(
                self,
                func: NonNull<vm::Func>,
                func_env: Option<NonNull<vm::FuncEnv>>,
                vmctx: Option<NonNull<vm::Ctx>>,
                wasm: Wasm,
            ) -> Result<Rets, RuntimeError>
            where
                Rets: WasmTypeList
            {
                #[allow(unused_parens)]
                let ( $( $x ),* ) = self;
                let args = [ $( $x.to_native().to_binary() ),* ];
                let mut rets = Rets::empty_ret_array();
                let mut trap = WasmTrapInfo::Unknown;
                let mut user_error = None;

                if (wasm.invoke)(
                    wasm.trampoline,
                    vmctx,
                    func_env,
                    func,
                    args.as_ptr(),
                    rets.as_mut().as_mut_ptr(),
                    &mut trap,
                    &mut user_error,
                    wasm.invoke_env
                ) {
                    Ok(Rets::from_ret_array(rets))
                } else {
                    if let Some(data) = user_error {
                        Err(RuntimeError::Error { data })
                    } else {
                        Err(RuntimeError::Trap { msg: trap.to_string().into() })
                    }
                }
            }
        }

        impl<'a $( , $x )*, Rets> Func<'a, ( $( $x ),* ), Rets, Wasm>
        where
            $( $x: WasmExternType, )*
            Rets: WasmTypeList,
        {
            #[allow(non_snake_case)]
            pub fn call(&self, $( $x: $x, )* ) -> Result<Rets, RuntimeError> {
                #[allow(unused_parens)]
                unsafe {
                    <( $( $x ),* ) as WasmTypeList>::call(
                        ( $( $x ),* ),
                        self.func,
                        self.func_env,
                        self.vmctx,
                        self.inner,
                    )
                }
            }
        }

        // Generic implementation for `Fn` (without `&mut Ctx` as first argument).
        impl< $( $x, )* Rets, Trap, FN > ExternalFunction<ClosedEnvironment, ( $( $x ),* ), Rets> for FN
        where
            $( $x: WasmExternType, )*
            Rets: WasmTypeList,
            Trap: TrapEarly<Rets>,
            FN: Fn( $( $x ),* ) -> Trap + 'static,
        {
            #[allow(non_snake_case)]
            fn to_raw(self) -> (NonNull<vm::Func>, Option<NonNull<vm::FuncEnv>>) {
                let env: Option<NonNull<vm::FuncEnv>> = if mem::size_of::<Self>() != 0 {
                    NonNull::new(Box::leak(Box::new(self))).map(|pointer| pointer.cast())
                } else {
                    None
                };

                /// This is required for the LLVM backend to be able to unwind through this function.
                #[cfg_attr(nightly, unwind(allowed))]
                extern fn wrap<$( $x, )* Rets, Trap, FN>(
                    env: *mut vm::FuncEnv
                    $( , $x: <$x as WasmExternType>::Native )*
                ) -> Rets::CStruct
                where
                    $( $x: WasmExternType, )*
                    Rets: WasmTypeList,
                    Trap: TrapEarly<Rets>,
                    FN: Fn( $( $x ),* ) -> Trap + 'static,
                {
                    let func: &FN = if mem::size_of::<FN>() != 0 {
                        unsafe { &*(env as *const FN) }
                    } else {
                        unsafe { mem::transmute(&()) }
                    };

                    let err = match panic::catch_unwind(
                        panic::AssertUnwindSafe(
                            || {
                                func( $( WasmExternType::from_native($x) ),* ).report()
                            }
                        )
                    ) {
                        Ok(Ok(returns)) => return returns.into_c_struct(),
                        Ok(Err(err)) => {
                            let b: Box<_> = err.into();
                            b as Box<dyn Any>
                        },
                        Err(err) => err,
                    };

                    unsafe {
                        let vmctx: &mut vm::Ctx = &mut *(env as *mut vm::Ctx);
                        (&*vmctx.module).runnable_module.do_early_trap(err)
                    }
                }

                (
                    NonNull::new(wrap::<$( $x, )* Rets, Trap, Self> as *mut vm::Func).unwrap(),
                    env,
                )
            }
        }

        // Specific implementation for `Fn` (with a `&mut Ctx` as first argument).
        impl< $( $x, )* Rets, Trap, FN > ExternalFunction<ExplicitVmCtx, ( $( $x ),* ), Rets> for FN
        where
            $( $x: WasmExternType, )*
            Rets: WasmTypeList,
            Trap: TrapEarly<Rets>,
            FN: Fn( &mut vm::Ctx $( , $x )* ) -> Trap + 'static,
        {
            #[allow(non_snake_case)]
            fn to_raw(self) -> (NonNull<vm::Func>, Option<NonNull<vm::FuncEnv>>) {
                if mem::size_of::<Self>() == 0 {
                    /// This is required for the LLVM backend to be able to unwind through this function.
                    #[cfg_attr(nightly, unwind(allowed))]
                    extern fn wrap<$( $x, )* Rets, Trap, FN>(
                        vmctx: &mut vm::Ctx
                        $( , $x: <$x as WasmExternType>::Native )*
                    ) -> Rets::CStruct
                    where
                        $( $x: WasmExternType, )*
                        Rets: WasmTypeList,
                        Trap: TrapEarly<Rets>,
                        FN: Fn( &mut vm::Ctx $( , $x )* ) -> Trap + 'static,
                    {
                        let func: FN = unsafe { mem::transmute_copy(&()) };

                        let err = match panic::catch_unwind(
                            panic::AssertUnwindSafe(
                                || {
                                    func(vmctx $( , WasmExternType::from_native($x) )* ).report()
                                }
                            )
                        ) {
                            Ok(Ok(returns)) => return returns.into_c_struct(),
                            Ok(Err(err)) => {
                                let b: Box<_> = err.into();
                                b as Box<dyn Any>
                            },
                            Err(err) => err,
                        };

                        unsafe {
                            (&*vmctx.module).runnable_module.do_early_trap(err)
                        }
                    }

                    (
                        NonNull::new(wrap::<$( $x, )* Rets, Trap, Self> as *mut vm::Func).unwrap(),
                        None,
                    )
                } else {
                    panic!("yolo");
                }

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
impl_traits!([transparent] S1, A);
impl_traits!([C] S2, A, B);
impl_traits!([C] S3, A, B, C);
impl_traits!([C] S4, A, B, C, D);
impl_traits!([C] S5, A, B, C, D, E);
impl_traits!([C] S6, A, B, C, D, E, F);
impl_traits!([C] S7, A, B, C, D, E, F, G);
impl_traits!([C] S8, A, B, C, D, E, F, G, H);
impl_traits!([C] S9, A, B, C, D, E, F, G, H, I);
impl_traits!([C] S10, A, B, C, D, E, F, G, H, I, J);
impl_traits!([C] S11, A, B, C, D, E, F, G, H, I, J, K);
impl_traits!([C] S12, A, B, C, D, E, F, G, H, I, J, K, L);

impl<'a, Args, Rets, Inner> IsExport for Func<'a, Args, Rets, Inner>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
    Inner: Kind,
{
    fn to_export(&self) -> Export {
        let func = unsafe { FuncPointer::new(self.func.as_ptr()) };
        let ctx = if let Some(ptr) = self.func_env {
            Context::External(ptr.cast().as_ptr())
        } else {
            Context::Internal
        };
        let signature = Arc::new(FuncSig::new(Args::types(), Rets::types()));

        Export::Function {
            func,
            ctx,
            signature,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_call() {
        fn foo(a: i32, b: i32) -> (i32, i32) {
            (a, b)
        }

        let _ = Func::new(foo);
    }

    #[test]
    fn test_imports() {
        use crate::{func, imports};

        fn foo(a: i32) -> i32 {
            a
        }

        let _ = imports! {
            "env" => {
                "foo" => func!(foo),
            },
        };
    }
}
