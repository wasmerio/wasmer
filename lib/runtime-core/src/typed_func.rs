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
    marker::PhantomData,
    mem, panic,
    ptr::{self, NonNull},
    sync::Arc,
};

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
    error_out: *mut Option<Box<dyn Any + Send>>,
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

impl Kind for Wasm {}

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
    /// directly in the code, see the `Func::call` implementation.
    unsafe fn call<Rets>(
        self,
        f: NonNull<vm::Func>,
        wasm: Wasm,
        ctx: *mut vm::Ctx,
    ) -> Result<Rets, RuntimeError>
    where
        Rets: WasmTypeList;
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

/// This empty structure indicates that an external function must
/// contain an explicit `vm::Ctx` argument (at first position).
///
/// ```rs,ignore
/// fn add_one(_: mut &vm::Ctx, x: i32) -> i32 {
///     x + 1
/// }
/// ```
#[doc(hidden)]
pub struct ExplicitVmCtx {}

impl HostFunctionKind for ExplicitVmCtx {}

/// This empty structure indicates that an external function has no
/// `vm::Ctx` argument (at first position). Its signature is:
///
/// ```rs,ignore
/// fn add_one(x: i32) -> i32 {
///     x + 1
/// }
/// ```
pub struct ImplicitVmCtx {}

impl HostFunctionKind for ImplicitVmCtx {}

/// Represents a function that can be converted to a `vm::Func`
/// (function pointer) that can be called within WebAssembly.
pub trait HostFunction<Kind, Args, Rets>
where
    Kind: HostFunctionKind,
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    /// Convert to function pointer.
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

/// Represents a type-erased function provided by either the host or the WebAssembly program.
pub struct DynamicFunc<'a> {
    _inner: Box<dyn Kind>,

    /// The function pointer.
    func: NonNull<vm::Func>,

    /// The function environment.
    func_env: Option<NonNull<vm::FuncEnv>>,

    /// The famous `vm::Ctx`.
    vmctx: *mut vm::Ctx,

    /// The runtime signature of this function.
    ///
    /// When converted from a `Func`, this is determined by the static `Args` and `Rets` type parameters.
    /// otherwise the signature is dynamically assigned during `DynamicFunc` creation, usually when creating
    /// a polymorphic host function.
    signature: Arc<FuncSig>,

    _phantom: PhantomData<&'a ()>,
}

unsafe impl<'a> Send for DynamicFunc<'a> {}

/// Represents a function that can be used by WebAssembly.
pub struct Func<'a, Args = (), Rets = (), Inner: Kind = Wasm> {
    inner: Inner,

    /// The function pointer.
    func: NonNull<vm::Func>,

    /// The function environment.
    func_env: Option<NonNull<vm::FuncEnv>>,

    /// The famous `vm::Ctx`.
    vmctx: *mut vm::Ctx,

    _phantom: PhantomData<(&'a (), Args, Rets)>,
}

unsafe impl<'a, Args, Rets> Send for Func<'a, Args, Rets, Wasm> {}
unsafe impl<'a, Args, Rets> Send for Func<'a, Args, Rets, Host> {}

impl<'a, Args, Rets, Inner> From<Func<'a, Args, Rets, Inner>> for DynamicFunc<'a>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
    Inner: Kind + 'static,
{
    fn from(that: Func<'a, Args, Rets, Inner>) -> DynamicFunc<'a> {
        DynamicFunc {
            _inner: Box::new(that.inner),
            func: that.func,
            func_env: that.func_env,
            vmctx: that.vmctx,
            signature: Arc::new(FuncSig::new(Args::types(), Rets::types())),
            _phantom: PhantomData,
        }
    }
}

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
}

impl<'a, Args, Rets> Func<'a, Args, Rets, Host>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    /// Creates a new `Func`.
    pub fn new<F, Kind>(func: F) -> Self
    where
        Kind: HostFunctionKind,
        F: HostFunction<Kind, Args, Rets>,
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

impl<'a> DynamicFunc<'a> {
    /// Creates a dynamic function that is polymorphic over its argument and return types.
    #[allow(unused_variables)]
    #[cfg(all(unix, target_arch = "x86_64"))]
    pub fn new<F>(signature: Arc<FuncSig>, func: F) -> Self
    where
        F: Fn(&mut vm::Ctx, &[crate::types::Value]) -> Vec<crate::types::Value> + 'static,
    {
        use crate::trampoline_x64::{CallContext, TrampolineBufferBuilder};
        use crate::types::Value;
        use std::convert::TryFrom;

        struct PolymorphicContext {
            arg_types: Vec<Type>,
            func: Box<dyn Fn(&mut vm::Ctx, &[Value]) -> Vec<Value>>,
        }
        unsafe extern "C" fn enter_host_polymorphic(
            ctx: *const CallContext,
            args: *const u64,
        ) -> u64 {
            let ctx = &*(ctx as *const PolymorphicContext);
            let vmctx = &mut *(*args.offset(0) as *mut vm::Ctx);
            let args: Vec<Value> = ctx
                .arg_types
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let i = i + 1; // skip vmctx
                    match *t {
                        Type::I32 => Value::I32(*args.offset(i as _) as i32),
                        Type::I64 => Value::I64(*args.offset(i as _) as i64),
                        Type::F32 => Value::F32(f32::from_bits(*args.offset(i as _) as u32)),
                        Type::F64 => Value::F64(f64::from_bits(*args.offset(i as _) as u64)),
                        Type::V128 => {
                            todo!("enter_host_polymorphic: 128-bit types are not supported")
                        }
                    }
                })
                .collect();
            let rets = (ctx.func)(vmctx, &args);
            if rets.len() == 0 {
                0
            } else if rets.len() == 1 {
                u64::try_from(rets[0].to_u128()).expect(
                    "128-bit return value from polymorphic host functions is not yet supported",
                )
            } else {
                panic!(
                    "multiple return values from polymorphic host functions is not yet supported"
                );
            }
        }

        // Disable "fat" closures for possible future changes.
        if mem::size_of::<F>() != 0 {
            unimplemented!("DynamicFunc with captured environment is not yet supported");
        }

        let mut builder = TrampolineBufferBuilder::new();
        let ctx: Box<PolymorphicContext> = Box::new(PolymorphicContext {
            arg_types: signature.params().to_vec(),
            func: Box::new(func),
        });
        let ctx = Box::into_raw(ctx);
        builder.add_callinfo_trampoline(
            enter_host_polymorphic,
            ctx as *const _,
            (signature.params().len() + 1) as u32, // +vmctx
        );
        let ptr = builder
            .insert_global()
            .expect("cannot bump-allocate global trampoline memory");

        struct AutoRelease {
            ptr: NonNull<u8>,
            ctx: *mut PolymorphicContext,
        }

        impl Drop for AutoRelease {
            fn drop(&mut self) {
                unsafe {
                    TrampolineBufferBuilder::remove_global(self.ptr);
                    drop(Box::from_raw(self.ctx));
                }
            }
        }

        impl Kind for AutoRelease {}

        DynamicFunc {
            _inner: Box::new(AutoRelease { ptr, ctx }),
            func: ptr.cast::<vm::Func>(),
            func_env: None,
            vmctx: ptr::null_mut(),
            signature,
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

    /// Get the underlying func pointer.
    pub fn get_vm_func(&self) -> NonNull<vm::Func> {
        self.func
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

        #[allow(unused_parens)]
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
                let mut error_out = None;

                if (wasm.invoke)(
                    wasm.trampoline,
                    ctx,
                    f,
                    args.as_ptr(),
                    rets.as_mut().as_mut_ptr(),
                    &mut error_out,
                    wasm.invoke_env
                ) {
                    Ok(Rets::from_ret_array(rets))
                } else {
                    Err(error_out.map(RuntimeError).unwrap_or_else(|| {
                        RuntimeError(Box::new("invoke(): Unknown error".to_string()))
                    }))
                }
            }
        }

        #[allow(unused_parens)]
        impl< $( $x, )* Rets, Trap, FN > HostFunction<ExplicitVmCtx, ( $( $x ),* ), Rets> for FN
        where
            $( $x: WasmExternType, )*
            Rets: WasmTypeList,
            Trap: TrapEarly<Rets>,
            FN: Fn(&mut vm::Ctx $( , $x )*) -> Trap + 'static + Send,
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

        #[allow(unused_parens)]
        impl< $( $x, )* Rets, Trap, FN > HostFunction<ImplicitVmCtx, ( $( $x ),* ), Rets> for FN
        where
            $( $x: WasmExternType, )*
            Rets: WasmTypeList,
            Trap: TrapEarly<Rets>,
            FN: Fn($( $x, )*) -> Trap + 'static + Send,
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

        #[allow(unused_parens)]
        impl<'a $( , $x )*, Rets> Func<'a, ( $( $x ),* ), Rets, Wasm>
        where
            $( $x: WasmExternType, )*
            Rets: WasmTypeList,
        {
            /// Call the typed func and return results.
            #[allow(non_snake_case, clippy::too_many_arguments)]
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

impl<'a> IsExport for DynamicFunc<'a> {
    fn to_export(&self) -> Export {
        let func = unsafe { FuncPointer::new(self.func.as_ptr()) };
        let ctx = match self.func_env {
            func_env @ Some(_) => Context::ExternalWithEnv(self.vmctx, func_env),
            None => Context::Internal,
        };

        Export::Function {
            func,
            ctx,
            signature: self.signature.clone(),
        }
    }
}

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

        Export::Function {
            func,
            ctx,
            signature: Arc::new(FuncSig::new(Args::types(), Rets::types())),
        }
    }
}

/// Function that always fails. It can be used as a placeholder when a
/// host function is missing for instance.
pub(crate) fn always_trap() -> Result<(), &'static str> {
    Err("not implemented")
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

    #[test]
    fn test_many_new_dynamics() {
        use crate::types::{FuncSig, Type};

        // Check that generating a lot (1M) of polymorphic functions doesn't use up the executable buffer.
        for _ in 0..1000000 {
            let arglist = vec![Type::I32; 100];
            DynamicFunc::new(
                Arc::new(FuncSig::new(arglist, vec![Type::I32])),
                |_, _| unreachable!(),
            );
        }
    }
}
