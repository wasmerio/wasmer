//! The typed func module implements a way of representing a wasm function
//! with the correct types from rust. Function calls using a typed func have a low overhead.
use crate::{
    backing::ImportBacking,
    error::RuntimeError,
    export::{Context, Export, FuncPointer},
    import::IsExport,
    module::ModuleInner,
    types::{FuncSig, ImportedFuncIndex, NativeWasmType, Type, Value, WasmExternType},
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

/// Empty trait to specify the kind of `HostFunction`: With or
/// without a `vm::Ctx` argument. See the `ExplicitVmCtx` and the
/// `ImplicitVmCtx` structures.
///
/// This type is never aimed to be used by a user. It is used by the
/// trait system to automatically generate an appropriate `wrap`
/// function.
pub trait HostFunctionKind {}

/// This empty structure indicates that an external function must
/// contain an explicit `vm::Ctx` argument (at first position).
///
/// ```rs,ignore
/// fn add_one(_: mut &vm::Ctx, x: i32) -> i32 {
///     x + 1
/// }
/// ```
pub struct ExplicitVmCtx;
impl HostFunctionKind for ExplicitVmCtx {}

/// This empty structure indicates that an external function has no
/// `vm::Ctx` argument (at first position). Its signature is:
///
/// ```rs,ignore
/// fn add_one(x: i32) -> i32 {
///     x + 1
/// }
/// ```
pub struct ImplicitVmCtx;
impl HostFunctionKind for ImplicitVmCtx {}

pub trait Arity {}

macro_rules! arity {
    ($($arity:ident),*) => {
        $(
            #[derive(Debug)]
            pub struct $arity;

            impl Arity for $arity {}
        )*
    }
}

arity!(Zero, One, Two, Three, Four, Five, Six, Seven, Eight, Nine, Ten, Eleven, Twelve);

/// Represents a function that can be converted to a `vm::Func`
/// (function pointer) that can be called within WebAssembly.
pub trait HostFunction<Kind, Args, Rets>
where
    Kind: HostFunctionKind,
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    /// Conver to function pointer.
    fn to_raw(self) -> (NonNull<vm::Func>, Option<NonNull<vm::FuncEnv>>);
}

pub trait VariadicHostFunction<Kind, A, Rets>
where
    Kind: HostFunctionKind,
    A: Arity,
    Rets: WasmTypeList,
{
    fn to_raw(self) -> (NonNull<vm::Func>, Option<NonNull<vm::FuncEnv>>);
}

/// Represents a TrapEarly type.
pub trait TrapEarly<Rets>
where
    Rets: WasmTypeList,
{
    /// The error type for this trait.
    type Error: 'static;

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
    E: 'static,
{
    type Error = E;

    fn report(self) -> Result<Rets, Self::Error> {
        self
    }
}

/// Represents a function that can be used by WebAssembly.
pub struct Func<'a, Args = (), Rets = (), Inner: Kind = Wasm> {
    inner: Inner,
    func: NonNull<vm::Func>,
    func_env: Option<NonNull<vm::FuncEnv>>,
    signature: Option<Arc<FuncSig>>,
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
        signature: Option<Arc<FuncSig>>,
        vmctx: *mut vm::Ctx,
    ) -> Func<'a, Args, Rets, Wasm> {
        Func {
            inner,
            func,
            func_env,
            signature,
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
            signature: None,
            vmctx: ptr::null_mut(),
            _phantom: PhantomData,
        }
    }
}

impl<'a, Rets> Func<'a, (), Rets, Host>
where
    Rets: WasmTypeList,
{
    /// Creates a new `Func` with a specific signature.
    pub fn new_variadic<F, Trap>(func: F, arity: i8, signature: Arc<FuncSig>) -> Self
    where
        Trap: TrapEarly<Rets>,
        F: Fn(&[Value]) -> Trap + 'static,
    {
        match arity {
            0 => Self::new_variadic_resolved::<Zero, _, _>(func, signature),
            1 => Self::new_variadic_resolved::<One, _, _>(func, signature),
            2 => Self::new_variadic_resolved::<Two, _, _>(func, signature),
            3 => Self::new_variadic_resolved::<Three, _, _>(func, signature),
            4 => Self::new_variadic_resolved::<Four, _, _>(func, signature),
            5 => Self::new_variadic_resolved::<Five, _, _>(func, signature),
            6 => Self::new_variadic_resolved::<Six, _, _>(func, signature),
            7 => Self::new_variadic_resolved::<Seven, _, _>(func, signature),
            8 => Self::new_variadic_resolved::<Eight, _, _>(func, signature),
            9 => Self::new_variadic_resolved::<Nine, _, _>(func, signature),
            10 => Self::new_variadic_resolved::<Ten, _, _>(func, signature),
            11 => Self::new_variadic_resolved::<Eleven, _, _>(func, signature),
            12 => Self::new_variadic_resolved::<Twelve, _, _>(func, signature),
            _ => unimplemented!("Host function can have 12 arguments maximum."),
        }
    }

    fn new_variadic_resolved<A, F, Kind>(
        func: F,
        signature: Arc<FuncSig>,
    ) -> Func<'a, (), Rets, Host>
    where
        A: Arity,
        Kind: HostFunctionKind,
        F: VariadicHostFunction<Kind, A, Rets>,
    {
        let (func, func_env) = func.to_raw();

        Func {
            inner: Host(()),
            func,
            func_env,
            signature: Some(signature),
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

/// Helper to get the real host function to call.
// It is used in the `to_raw` -> `wrap` functions, and it should
// remain like this :-).
#[inline(always)]
fn wrap_get_func<'f, FN>(
    import_backing: *const ImportBacking,
    self_pointer: *const vm::Func,
) -> (&'f FN, ImportedFuncIndex) {
    // Get the collection of imported functions.
    let vm_imported_functions = unsafe { &(*import_backing).vm_functions };

    // Retrieve the `vm::FuncCtx`.
    let (mut func_ctx, index): (NonNull<vm::FuncCtx>, ImportedFuncIndex) = vm_imported_functions
        .iter()
        .find_map(|(_, imported_func)| {
            if imported_func.func == self_pointer {
                Some((imported_func.func_ctx, imported_func.index))
            } else {
                None
            }
        })
        .expect("Import backing is not well-formed, cannot find `func_ctx`.");
    let func_ctx = unsafe { func_ctx.as_mut() };

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
        None => unreachable!(),
    };

    (func, index)
}

/// Helper to call a host function safely.
// It is used in the `to_raw` -> `wrap` functions, and it should
// remain like this :-).
#[inline(always)]
fn wrap_call<Rets, Trap>(
    module: *const ModuleInner,
    executor: &(dyn Fn() -> Result<Rets, Trap::Error>),
) -> Rets::CStruct
where
    Rets: WasmTypeList,
    Trap: TrapEarly<Rets>,
{
    // Catch unwind in case of errors.
    let err = match panic::catch_unwind(panic::AssertUnwindSafe(executor)) {
        Ok(Ok(returns)) => return returns.into_c_struct(),
        Ok(Err(err)) => {
            let b: Box<_> = err.into();
            b as Box<dyn Any>
        }
        Err(err) => err,
    };

    // At this point, there is an error that needs to
    // be trapped.
    unsafe { (&*module).runnable_module.do_early_trap(err) }
}

/// Helper to get function environment pointer of a host function.
// It is used in the `to_raw` functions, and it should remain like
// this :-).
#[inline(always)]
fn get_func_env<FN>(func: FN) -> Option<NonNull<vm::FuncEnv>> {
    // `FN` is a function pointer, or a closure
    // _without_ a captured environment.
    if mem::size_of::<FN>() == 0 {
        NonNull::new(&func as *const _ as *mut vm::FuncEnv)
    }
    // `FN` is a closure _with_ a captured
    // environment.
    else {
        NonNull::new(Box::into_raw(Box::new(func))).map(NonNull::cast)
    }
}

macro_rules! impl_traits {
    ( [$repr:ident] $struct_name:ident, $arity:ident, $( $x:ident ),* ) => {
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

            #[allow(non_snake_case)]
            fn from_ret_array(array: Self::RetArray) -> Self {
                let [ $( $x ),* ] = array;

                ( $( WasmExternType::from_native(NativeWasmType::from_binary($x)) ),* )
            }

            fn empty_ret_array() -> Self::RetArray {
                [0; count_idents!( $( $x ),* )]
            }

            #[allow(non_snake_case)]
            fn from_c_struct(c_struct: Self::CStruct) -> Self {
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

        impl< $( $x, )* Rets, Trap, FN > HostFunction<ExplicitVmCtx, ( $( $x ),* ), Rets> for FN
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
                    vmctx: &mut vm::Ctx $( , $x: <$x as WasmExternType>::Native )*
                ) -> Rets::CStruct
                where
                    $( $x: WasmExternType, )*
                    Rets: WasmTypeList,
                    Trap: TrapEarly<Rets>,
                    FN: Fn(&mut vm::Ctx, $( $x, )*) -> Trap,
                {
                    // Get the pointer to this `wrap` function.
                    let self_pointer = wrap::<$( $x, )* Rets, Trap, FN> as *const vm::Func;

                    // Get the real host function to call.
                    let (func, _): (&FN, _) = wrap_get_func(vmctx.import_backing, self_pointer);

                    wrap_call::<Rets, Trap>(
                        vmctx.module,
                        &|| {
                            let vmctx = unsafe { &mut *(vmctx as *const _ as *mut _) };

                            func(vmctx $( , WasmExternType::from_native($x) )* ).report()
                            //   ^^^^^ The imported function
                            //         expects `vm::Ctx` as first
                            //         argument; provide it.
                        }
                    )
                }

                (
                    NonNull::new(wrap::<$( $x, )* Rets, Trap, Self> as *mut vm::Func).unwrap(),
                    get_func_env(self)
                )
            }
        }

        impl< $( $x, )* Rets, Trap, FN > HostFunction<ImplicitVmCtx, ( $( $x ),* ), Rets> for FN
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
                    vmctx: &mut vm::Ctx $( , $x: <$x as WasmExternType>::Native )*
                ) -> Rets::CStruct
                where
                    $( $x: WasmExternType, )*
                    Rets: WasmTypeList,
                    Trap: TrapEarly<Rets>,
                    FN: Fn($( $x, )*) -> Trap,
                {
                    // Get the pointer to this `wrap` function.
                    let self_pointer = wrap::<$( $x, )* Rets, Trap, FN> as *const vm::Func;

                    // Get the real host function to call.
                    let (func, _): (&FN, _) = wrap_get_func(vmctx.import_backing, self_pointer);

                    // Call the host function.
                    wrap_call::<Rets, Trap>(
                        vmctx.module,
                        &|| {
                            func($( WasmExternType::from_native($x) ),* ).report()
                        }
                    )
                }

                (
                    NonNull::new(wrap::<$( $x, )* Rets, Trap, Self> as *mut vm::Func).unwrap(),
                    get_func_env(self)
                )
            }
        }

        impl<Rets, Trap, FN> VariadicHostFunction<ImplicitVmCtx, $arity, Rets> for FN
        where
            Rets: WasmTypeList,
            Trap: TrapEarly<Rets>,
            FN: Fn(&[Value]) -> Trap + 'static,
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
                extern fn wrap<Rets, Trap, FN>(
                    vmctx: &mut vm::Ctx $( , $x: u64 )*
                ) -> Rets::CStruct
                where
                    Rets: WasmTypeList,
                    Trap: TrapEarly<Rets>,
                    FN: Fn(&[Value]) -> Trap,
                {
                    // Get the pointer to this `wrap` function.
                    let self_pointer = wrap::<Rets, Trap, FN> as *const vm::Func;

                    // Get the real host function to call.
                    let (func, func_index): (&FN, ImportedFuncIndex) = wrap_get_func(vmctx.import_backing, self_pointer);

                    // Read the signature.
                    let module_info = &unsafe{ &*vmctx.module }.info;
                    let func_signature_index = module_info.func_assoc[func_index.convert_up(&module_info)];
                    let func_signature = &module_info.signatures[func_signature_index];

                    // Store all arguments in a single array.
                    let arguments = &[ $($x),* ];

                    // Call the host function.
                    wrap_call::<Rets, Trap>(
                        vmctx.module,
                        &|| {
                            // Map `u64` to `Value` based on the signature.
                            let inputs: Vec<Value> = arguments
                                .iter()
                                .zip(func_signature.params().iter())
                                .map(
                                    |(argument, ty)| {
                                        match ty {
                                            Type::I32 => i32::from_binary(*argument).into(),
                                            Type::I64 => i64::from_binary(*argument).into(),
                                            Type::F32 => f32::from_binary(*argument).into(),
                                            Type::F64 => f64::from_binary(*argument).into(),
                                            _ => unimplemented!("Variadic host function doesn't support `v128` values."),
                                        }
                                    }
                                )
                                .collect();

                            func(inputs.as_slice()).report()
                        }
                    )
                }

                (
                    NonNull::new(wrap::<Rets, Trap, Self> as *mut vm::Func).unwrap(),
                    get_func_env(self)
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

impl_traits!([C] S0, Zero, );
impl_traits!([transparent] S1, One, A);
impl_traits!([C] S2, Two, A, B);
impl_traits!([C] S3, Three, A, B, C);
impl_traits!([C] S4, Four, A, B, C, D);
impl_traits!([C] S5, Five, A, B, C, D, E);
impl_traits!([C] S6, Six, A, B, C, D, E, F);
impl_traits!([C] S7, Seven, A, B, C, D, E, F, G);
impl_traits!([C] S8, Eight, A, B, C, D, E, F, G, H);
impl_traits!([C] S9, Nine, A, B, C, D, E, F, G, H, I);
impl_traits!([C] S10, Ten, A, B, C, D, E, F, G, H, I, J);
impl_traits!([C] S11, Eleven, A, B, C, D, E, F, G, H, I, J, K);
impl_traits!([C] S12, Twelve, A, B, C, D, E, F, G, H, I, J, K, L);

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
        let signature = match self.signature {
            Some(ref signature) => signature.clone(),
            None => Arc::new(FuncSig::new(Args::types(), Rets::types())),
        };

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
    //use std::convert::TryInto;

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

    #[test]
    fn test_func_variadic() {
        /*
        fn with_vmctx_variadic(_: &mut vm::Ctx, inputs: &[Value]) -> i32 {
            let x: i32 = (&inputs[0]).try_into().unwrap();
            let y: i32 = (&inputs[1]).try_into().unwrap();

            x + y
        }

        fn without_vmctx_variadic(inputs: &[Value]) -> i32 {
            let x: i32 = (&inputs[0]).try_into().unwrap();
            let y: i32 = (&inputs[1]).try_into().unwrap();

            x + y
        }

        let _: Func<(i32, i32), i32, Host> = Func::new_with_signature(
            with_vmctx_variadic,
            Arc::new(FuncSig::new(vec![Type::I32, Type::I32], vec![Type::I32])),
        );
        let _: Func<(i32, i32), i32, Host> = Func::new_with_signature(
            without_vmctx_variadic,
            Arc::new(FuncSig::new(vec![Type::I32, Type::I32], vec![Type::I32])),
        );
        let _: Func<(i32, i32), i32, Host> = Func::new_with_signature(
            |_: &mut vm::Ctx, inputs: &[Value]| -> i32 {
                let x: i32 = (&inputs[0]).try_into().unwrap();
                let y: i32 = (&inputs[1]).try_into().unwrap();

                x + y
            },
            Arc::new(FuncSig::new(vec![Type::I32, Type::I32], vec![Type::I32])),
        );
        */

        let _ = Func::new_with_signature(
            |_inputs: &[Value]| -> i32 {
                /*
                let x: i32 = (&inputs[0]).try_into().unwrap();
                let y: i32 = (&inputs[1]).try_into().unwrap();

                x + y
                 */
                42
            },
            Arc::new(FuncSig::new(vec![Type::I32, Type::I32], vec![Type::I32])),
        );
    }

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
