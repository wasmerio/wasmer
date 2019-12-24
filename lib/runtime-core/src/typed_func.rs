//! The typed func module implements a way of representing a wasm function
//! with the correct types from rust. Function calls using a typed func have a low overhead.
use crate::{
    error::RuntimeError,
    export::{Context, Export, FuncPointer},
    import::IsExport,
    types::{FuncSig, NativeWasmType, Type, WasmExternType},
    vm,
};
use std::{
    any::Any,
    convert::Infallible,
    ffi::c_void,
    fmt,
    marker::PhantomData,
    mem, panic,
    ptr::{self, NonNull},
    sync::Arc,
};

/// Wasm trap info.
#[repr(C)]
pub enum WasmTrapInfo {
    /// Unreachable trap.
    Unreachable = 0,
    /// Call indirect incorrect signature trap.
    IncorrectCallIndirectSignature = 1,
    /// Memory out of bounds trap.
    MemoryOutOfBounds = 2,
    /// Call indirect out of bounds trap.
    CallIndirectOOB = 3,
    /// Illegal arithmetic trap.
    IllegalArithmetic = 4,
    /// Misaligned atomic access trap.
    MisalignedAtomicAccess = 5,
    /// Unknown trap.
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

/// Aliases to an extern "C" type used as a trampoline to a function.
pub type Trampoline = unsafe extern "C" fn(
    vmctx: *mut vm::Ctx,
    func: NonNull<vm::Func>,
    args: *const u64,
    rets: *mut u64,
);

/// Aliases to an extern "C" type used to invoke a function.
pub type Invoke = unsafe extern "C" fn(
    trampoline: Trampoline,
    vmctx: *mut vm::Ctx,
    func: NonNull<vm::Func>,
    args: *const u64,
    rets: *mut u64,
    trap_info: *mut WasmTrapInfo,
    user_error: *mut Option<Box<dyn Any + Send>>,
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
    /// Create new `Wasm` from given parts.
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
    /// CStruct type.
    type CStruct;

    /// Array of return values.
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
        f: NonNull<vm::Func>,
        wasm: Wasm,
        ctx: *mut vm::Ctx,
    ) -> Result<Rets, RuntimeError>
    where
        Rets: WasmTypeList;
}

/// Empty trait to specify the kind of `ExternalFunction`: With or
/// without a `vm::Ctx` argument. See the `ExplicitVmCtx` and the
/// `ImplicitVmCtx` structures.
///
/// This type is never aimed to be used by a user. It is used by the
/// trait system to automatically generate an appropriate `wrap`
/// function.
pub trait ExternalFunctionKind {}

/// This empty structure indicates that an external function must
/// contain an explicit `vm::Ctx` argument (at first position).
///
/// ```rs,ignore
/// fn add_one(_: mut &vm::Ctx, x: i32) -> i32 {
///     x + 1
/// }
/// ```
pub struct ExplicitVmCtx {}

/// This empty structure indicates that an external function has no
/// `vm::Ctx` argument (at first position). Its signature is:
///
/// ```rs,ignore
/// fn add_one(x: i32) -> i32 {
///     x + 1
/// }
/// ```
pub struct ImplicitVmCtx {}

impl ExternalFunctionKind for ExplicitVmCtx {}
impl ExternalFunctionKind for ImplicitVmCtx {}

/// Represents a function that can be converted to a `vm::Func`
/// (function pointer) that can be called within WebAssembly.
pub trait ExternalFunction<Kind, Args, Rets>
where
    Kind: ExternalFunctionKind,
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    /// Conver to function pointer.
    fn to_raw(self) -> (NonNull<vm::Func>, Option<NonNull<vm::FuncEnv>>);
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

/// Represents a function that can be used by WebAssembly.
pub struct Func<'a, Args = (), Rets = (), Inner: Kind = Wasm> {
    inner: Inner,
    func: NonNull<vm::Func>,
    func_env: Option<NonNull<vm::FuncEnv>>,
    vmctx: *mut vm::Ctx,
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
        vmctx: *mut vm::Ctx,
    ) -> Func<'a, Args, Rets, Wasm> {
        Func {
            inner,
            func,
            func_env,
            vmctx,
            _phantom: PhantomData,
        }
    }

    /// Get the underlying func pointer.
    pub fn get_vm_func(&self) -> NonNull<vm::Func> {
        self.func
    }
}

impl<'a, Args, Rets> Func<'a, Args, Rets, Host>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    /// Creates a new `Func`.
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
            vmctx: ptr::null_mut(),
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
        _: Wasm,
        _: *mut vm::Ctx,
    ) -> Result<Rets, RuntimeError>
    where
        Rets: WasmTypeList,
    {
        unreachable!()
    }
}

macro_rules! impl_traits {
    ( [$repr:ident] $struct_name:ident, $( $x:ident ),* ) => {
        /// Struct for typed funcs.
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

            #[allow(unused_parens, non_snake_case)]
            fn into_c_struct(self) -> Self::CStruct {
                let ( $( $x ),* ) = self;

                $struct_name ( $( WasmExternType::to_native($x) ),* )
            }

            fn types() -> &'static [Type] {
                &[$( $x::Native::TYPE ),*]
            }

            #[allow(unused_parens, non_snake_case)]
            unsafe fn call<Rets>(
                self,
                f: NonNull<vm::Func>,
                wasm: Wasm,
                ctx: *mut vm::Ctx,
            ) -> Result<Rets, RuntimeError>
            where
                Rets: WasmTypeList
            {
                let ( $( $x ),* ) = self;
                let args = [ $( $x.to_native().to_binary()),* ];
                let mut rets = Rets::empty_ret_array();
                let mut trap = WasmTrapInfo::Unknown;
                let mut user_error = None;

                if (wasm.invoke)(
                    wasm.trampoline,
                    ctx,
                    f,
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

        impl< $( $x, )* Rets, Trap, FN > ExternalFunction<ExplicitVmCtx, ( $( $x ),* ), Rets> for FN
        where
            $( $x: WasmExternType, )*
            Rets: WasmTypeList,
            Trap: TrapEarly<Rets>,
            FN: Fn(&mut vm::Ctx $( , $x )*) -> Trap + 'static,
        {
            #[allow(non_snake_case)]
            fn to_raw(self) -> (NonNull<vm::Func>, Option<NonNull<vm::FuncEnv>>) {
                // The `wrap` function is a wrapper around the
                // imported function. It manages the argument passed
                // to the imported function (in this case, the
                // `vmctx` along with the regular WebAssembly
                // arguments), and it manages the trapping.
                //
                // It is also required for the LLVM backend to be
                // able to unwind through this function.
                #[cfg_attr(nightly, unwind(allowed))]
                extern fn wrap<$( $x, )* Rets, Trap, FN>(
                    vmctx: &vm::Ctx $( , $x: <$x as WasmExternType>::Native )*
                ) -> Rets::CStruct
                where
                    $( $x: WasmExternType, )*
                    Rets: WasmTypeList,
                    Trap: TrapEarly<Rets>,
                    FN: Fn(&mut vm::Ctx, $( $x, )*) -> Trap,
                {
                    // Get the pointer to this `wrap` function.
                    let self_pointer = wrap::<$( $x, )* Rets, Trap, FN> as *const vm::Func;

                    // Get the collection of imported functions.
                    let vm_imported_functions = unsafe { &(*vmctx.import_backing).vm_functions };

                    // Retrieve the `vm::FuncCtx`.
                    let mut func_ctx: NonNull<vm::FuncCtx> = vm_imported_functions
                        .iter()
                        .find_map(|(_, imported_func)| {
                            if imported_func.func == self_pointer {
                                Some(imported_func.func_ctx)
                            } else {
                                None
                            }
                        })
                        .expect("Import backing is not well-formed, cannot find `func_ctx`.");
                    let func_ctx = unsafe { func_ctx.as_mut() };

                    // Extract `vm::Ctx` from `vm::FuncCtx`. The
                    // pointer is always non-null.
                    let vmctx = unsafe { func_ctx.vmctx.as_mut() };

                    // Extract `vm::FuncEnv` from `vm::FuncCtx`.
                    let func_env = func_ctx.func_env;

                    let func: &FN = match func_env {
                        // The imported function is a regular
                        // function, a closure without a captured
                        // environment, or a closure with a captured
                        // environment.
                        Some(func_env) => unsafe {
                            let func: NonNull<FN> = func_env.cast();

                            &*func.as_ptr()
                        },

                        // This branch is supposed to be unreachable.
                        None => unreachable!()
                    };

                    // Catch unwind in case of errors.
                    let err = match panic::catch_unwind(
                        panic::AssertUnwindSafe(
                            || {
                                func(vmctx $( , WasmExternType::from_native($x) )* ).report()
                                //   ^^^^^ The imported function
                                //         expects `vm::Ctx` as first
                                //         argument; provide it.
                            }
                        )
                    ) {
                        Ok(Ok(returns)) => return returns.into_c_struct(),
                        Ok(Err(err)) => {
                            let b: Box<_> = err.into();
                            b as Box<dyn Any + Send>
                        },
                        Err(err) => err,
                    };

                    // At this point, there is an error that needs to
                    // be trapped.
                    unsafe {
                        (&*vmctx.module).runnable_module.do_early_trap(err)
                    }
                }

                // Extract the captured environment of the imported
                // function if any.
                let func_env: Option<NonNull<vm::FuncEnv>> =
                    // `FN` is a function pointer, or a closure
                    // _without_ a captured environment.
                    if mem::size_of::<Self>() == 0 {
                        NonNull::new(&self as *const _ as *mut vm::FuncEnv)
                    }
                    // `FN` is a closure _with_ a captured
                    // environment.
                    else {
                        NonNull::new(Box::into_raw(Box::new(self))).map(NonNull::cast)
                    };

                (
                    NonNull::new(wrap::<$( $x, )* Rets, Trap, Self> as *mut vm::Func).unwrap(),
                    func_env
                )
            }
        }

        impl< $( $x, )* Rets, Trap, FN > ExternalFunction<ImplicitVmCtx, ( $( $x ),* ), Rets> for FN
        where
            $( $x: WasmExternType, )*
            Rets: WasmTypeList,
            Trap: TrapEarly<Rets>,
            FN: Fn($( $x, )*) -> Trap + 'static,
        {
            #[allow(non_snake_case)]
            fn to_raw(self) -> (NonNull<vm::Func>, Option<NonNull<vm::FuncEnv>>) {
                // The `wrap` function is a wrapper around the
                // imported function. It manages the argument passed
                // to the imported function (in this case, only the
                // regular WebAssembly arguments), and it manages the
                // trapping.
                //
                // It is also required for the LLVM backend to be
                // able to unwind through this function.
                #[cfg_attr(nightly, unwind(allowed))]
                extern fn wrap<$( $x, )* Rets, Trap, FN>(
                    vmctx: &vm::Ctx $( , $x: <$x as WasmExternType>::Native )*
                ) -> Rets::CStruct
                where
                    $( $x: WasmExternType, )*
                    Rets: WasmTypeList,
                    Trap: TrapEarly<Rets>,
                    FN: Fn($( $x, )*) -> Trap,
                {
                    // Get the pointer to this `wrap` function.
                    let self_pointer = wrap::<$( $x, )* Rets, Trap, FN> as *const vm::Func;

                    // Get the collection of imported functions.
                    let vm_imported_functions = unsafe { &(*vmctx.import_backing).vm_functions };

                    // Retrieve the `vm::FuncCtx`.
                    let mut func_ctx: NonNull<vm::FuncCtx> = vm_imported_functions
                        .iter()
                        .find_map(|(_, imported_func)| {
                            if imported_func.func == self_pointer {
                                Some(imported_func.func_ctx)
                            } else {
                                None
                            }
                        })
                        .expect("Import backing is not well-formed, cannot find `func_ctx`.");
                    let func_ctx = unsafe { func_ctx.as_mut() };

                    // Extract `vm::Ctx` from `vm::FuncCtx`. The
                    // pointer is always non-null.
                    let vmctx = unsafe { func_ctx.vmctx.as_mut() };

                    // Extract `vm::FuncEnv` from `vm::FuncCtx`.
                    let func_env = func_ctx.func_env;

                    let func: &FN = match func_env {
                        // The imported function is a regular
                        // function, a closure without a captured
                        // environment, or a closure with a captured
                        // environment.
                        Some(func_env) => unsafe {
                            let func: NonNull<FN> = func_env.cast();

                            &*func.as_ptr()
                        },

                        // This branch is supposed to be unreachable.
                        None => unreachable!()
                    };

                    // Catch unwind in case of errors.
                    let err = match panic::catch_unwind(
                        panic::AssertUnwindSafe(
                            || {
                                func($( WasmExternType::from_native($x), )* ).report()
                            }
                        )
                    ) {
                        Ok(Ok(returns)) => return returns.into_c_struct(),
                        Ok(Err(err)) => {
                            let b: Box<_> = err.into();
                            b as Box<dyn Any + Send>
                        },
                        Err(err) => err,
                    };

                    // At this point, there is an error that needs to
                    // be trapped.
                    unsafe {
                        (&*vmctx.module).runnable_module.do_early_trap(err)
                    }
                }

                // Extract the captured environment of the imported
                // function if any.
                let func_env: Option<NonNull<vm::FuncEnv>> =
                    // `FN` is a function pointer, or a closure
                    // _without_ a captured environment.
                    if mem::size_of::<Self>() == 0 {
                        NonNull::new(&self as *const _ as *mut vm::FuncEnv)
                    }
                    // `FN` is a closure _with_ a captured
                    // environment.
                    else {
                        NonNull::new(Box::into_raw(Box::new(self))).map(NonNull::cast)
                    };

                (
                    NonNull::new(wrap::<$( $x, )* Rets, Trap, Self> as *mut vm::Func).unwrap(),
                    func_env
                )
            }
        }

        impl<'a $( , $x )*, Rets> Func<'a, ( $( $x ),* ), Rets, Wasm>
        where
            $( $x: WasmExternType, )*
            Rets: WasmTypeList,
        {
            /// Call the typed func and return results.
            #[allow(non_snake_case)]
            pub fn call(&self, $( $x: $x, )* ) -> Result<Rets, RuntimeError> {
                #[allow(unused_parens)]
                unsafe {
                    <( $( $x ),* ) as WasmTypeList>::call(
                        ( $( $x ),* ),
                        self.func,
                        self.inner,
                        self.vmctx
                    )
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
impl_traits!([C] S13, A, B, C, D, E, F, G, H, I, J, K, L, M);
impl_traits!([C] S14, A, B, C, D, E, F, G, H, I, J, K, L, M, N);
impl_traits!([C] S15, A, B, C, D, E, F, G, H, I, J, K, L, M, N, O);
impl_traits!([C] S16, A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P);
impl_traits!([C] S17, A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q);
impl_traits!([C] S18, A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R);
impl_traits!([C] S19, A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S);
impl_traits!([C] S20, A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T);
impl_traits!([C] S21, A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U);
impl_traits!([C] S22, A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V);
impl_traits!([C] S23, A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W);
impl_traits!([C] S24, A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X);
impl_traits!([C] S25, A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y);
impl_traits!([C] S26, A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z);

impl<'a, Args, Rets, Inner> IsExport for Func<'a, Args, Rets, Inner>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
    Inner: Kind,
{
    fn to_export(&self) -> Export {
        let func = unsafe { FuncPointer::new(self.func.as_ptr()) };
        let ctx = match self.func_env {
            func_env @ Some(_) => Context::ExternalWithEnv(self.vmctx, func_env),
            None => Context::Internal,
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

    macro_rules! test_func_arity_n {
        ($test_name:ident, $($x:ident),*) => {
            #[test]
            fn $test_name() {
                use crate::vm;

                fn with_vmctx(_: &mut vm::Ctx, $($x: i32),*) -> i32 {
                    vec![$($x),*].iter().sum()
                }

                fn without_vmctx($($x: i32),*) -> i32 {
                    vec![$($x),*].iter().sum()
                }

                let _ = Func::new(with_vmctx);
                let _ = Func::new(without_vmctx);
                let _ = Func::new(|_: &mut vm::Ctx, $($x: i32),*| -> i32 {
                    vec![$($x),*].iter().sum()
                });
                let _ = Func::new(|$($x: i32),*| -> i32 {
                    vec![$($x),*].iter().sum()
                });
            }
        }
    }

    #[test]
    fn test_func_arity_0() {
        fn foo(_: &mut vm::Ctx) -> i32 {
            0
        }

        fn bar() -> i32 {
            0
        }

        let _ = Func::new(foo);
        let _ = Func::new(bar);
        let _ = Func::new(|_: &mut vm::Ctx| -> i32 { 0 });
        let _ = Func::new(|| -> i32 { 0 });
    }

    test_func_arity_n!(test_func_arity_1, a);
    test_func_arity_n!(test_func_arity_2, a, b);
    test_func_arity_n!(test_func_arity_3, a, b, c);
    test_func_arity_n!(test_func_arity_4, a, b, c, d);
    test_func_arity_n!(test_func_arity_5, a, b, c, d, e);
    test_func_arity_n!(test_func_arity_6, a, b, c, d, e, f);
    test_func_arity_n!(test_func_arity_7, a, b, c, d, e, f, g);
    test_func_arity_n!(test_func_arity_8, a, b, c, d, e, f, g, h);
    test_func_arity_n!(test_func_arity_9, a, b, c, d, e, f, g, h, i);
    test_func_arity_n!(test_func_arity_10, a, b, c, d, e, f, g, h, i, j);
    test_func_arity_n!(test_func_arity_11, a, b, c, d, e, f, g, h, i, j, k);
    test_func_arity_n!(test_func_arity_12, a, b, c, d, e, f, g, h, i, j, k, l);
    test_func_arity_n!(test_func_arity_13, a, b, c, d, e, f, g, h, i, j, k, l, m);
    test_func_arity_n!(test_func_arity_14, a, b, c, d, e, f, g, h, i, j, k, l, m, n);
    #[rustfmt::skip] test_func_arity_n!(test_func_arity_15, a, b, c, d, e, f, g, h, i, j, k, l, m, n, o);
    #[rustfmt::skip] test_func_arity_n!(test_func_arity_16, a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p);
    #[rustfmt::skip] test_func_arity_n!(test_func_arity_17, a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q);
    #[rustfmt::skip] test_func_arity_n!(test_func_arity_18, a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r);
    #[rustfmt::skip] test_func_arity_n!(test_func_arity_19, a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r, s);
    #[rustfmt::skip] test_func_arity_n!(test_func_arity_20, a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r, s, t);
    #[rustfmt::skip] test_func_arity_n!(test_func_arity_21, a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r, s, t, u);
    #[rustfmt::skip] test_func_arity_n!(test_func_arity_22, a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r, s, t, u, v);
    #[rustfmt::skip] test_func_arity_n!(test_func_arity_23, a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r, s, t, u, v, w);
    #[rustfmt::skip] test_func_arity_n!(test_func_arity_24, a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r, s, t, u, v, w, x);
    #[rustfmt::skip] test_func_arity_n!(test_func_arity_25, a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r, s, t, u, v, w, x, y);
    #[rustfmt::skip] test_func_arity_n!(test_func_arity_26, a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r, s, t, u, v, w, x, y, z);

    #[test]
    fn test_call() {
        fn foo(_ctx: &mut vm::Ctx, a: i32, b: i32) -> (i32, i32) {
            (a, b)
        }

        let _f = Func::new(foo);
    }

    #[test]
    fn test_imports() {
        use crate::{func, imports};

        fn foo(_ctx: &mut vm::Ctx, a: i32) -> i32 {
            a
        }

        let _import_object = imports! {
            "env" => {
                "foo" => func!(foo),
            },
        };
    }
}
