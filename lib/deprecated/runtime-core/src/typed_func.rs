use crate::{
    error::{ExportError, RuntimeError},
    get_global_store, new,
    types::{FuncDescriptor, FuncSig, NativeWasmType, Type, Value, WasmExternType},
    vm,
};
use std::marker::PhantomData;

pub use new::wasmer::{HostFunction, WasmTypeList};

/// Represents a function that can be used by WebAssembly.
#[derive(Clone)]
pub struct Func<Args = (), Rets = ()>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    new_function: new::wasmer::Function,
    _phantom: PhantomData<(Args, Rets)>,
}

impl<Args, Rets> Func<Args, Rets>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    /// Creates a new `Func`.
    pub fn new<F>(func: F) -> Self
    where
        F: HostFunction<Args, Rets, new::wasmer::internals::WithEnv, vm::Ctx>,
    {
        // Create an empty `vm::Ctx`, that is going to be overwritten by `Instance::new`.
        let ctx = unsafe { vm::Ctx::new_uninit() };

        Self {
            new_function: new::wasmer::Function::new_env::<F, Args, Rets, vm::Ctx>(
                get_global_store(),
                ctx,
                func,
            ),
            _phantom: PhantomData,
        }
    }

    /// Returns the full function signature.
    pub fn signature(&self) -> &FuncDescriptor {
        self.new_function.ty()
    }

    /// Returns the types of the function inputs.
    pub fn params(&self) -> &[Type] {
        self.signature().params()
    }

    /// Returns the types of the function outputs.
    pub fn returns(&self) -> &[Type] {
        self.signature().results()
    }

    /// Call the function by passing all arguments in a slice of `Value`.
    pub fn dyn_call(&self, params: &[Value]) -> Result<Box<[Value]>, RuntimeError> {
        self.new_function.call(params)
    }
}

pub unsafe trait WasmExternTypeInner: WasmExternType
where
    Self: Sized,
{
}

unsafe impl WasmExternTypeInner for i8 {}
unsafe impl WasmExternTypeInner for u8 {}
unsafe impl WasmExternTypeInner for i16 {}
unsafe impl WasmExternTypeInner for u16 {}
unsafe impl WasmExternTypeInner for i32 {}
unsafe impl WasmExternTypeInner for u32 {}
unsafe impl WasmExternTypeInner for i64 {}
unsafe impl WasmExternTypeInner for u64 {}
unsafe impl WasmExternTypeInner for f32 {}
unsafe impl WasmExternTypeInner for f64 {}

macro_rules! func_call {
    ( $( $x:ident ),* ) => {
        #[allow(unused_parens)]
        impl< $( $x, )* Rets: WasmTypeList > Func<( $( $x ),* ), Rets>
        where
            $( $x: WasmExternType + WasmExternTypeInner, )*
            Rets: WasmTypeList
        {
            /// Call the function.
            #[allow(non_snake_case, clippy::too_many_arguments)]
            pub fn call(&self, $( $x: $x, )* ) -> Result<Rets, RuntimeError> {
                // Two implementation choices:
                //   1. Either by using the `NativeFunc` API, but a
                //      new native function must be created for each
                //      call,
                //   2. Pack the parameters into a slice, call
                //      `dyn_call` with it, and unpack the results.
                //
                // The first implementation is the following:
                //
                // self.new_function.native::<( $( $x ),* ), Rets>().unwrap().call( $( $x ),* )
                //
                // The second implementation is the following active one:

                // Pack the argument into a slice.
                let params: &[Value] = &[
                    $(
                        $x.to_native().to_value()
                    ),*
                ];

                // Call the function with `dyn_call`, and transform the results into a vector.
                let results: Vec<Value> = self.dyn_call(params)?.to_vec();

                // Map the results into their binary form.
                let rets: Vec<i128> = results
                    .iter()
                    .map(|value| {
                        Ok(match value {
                            Value::I32(value) => <i32 as WasmExternType>::from_native(*value).to_binary(),
                            Value::I64(value) => <i64 as WasmExternType>::from_native(*value).to_binary(),
                            Value::F32(value) => <f32 as WasmExternType>::from_native(*value).to_binary(),
                            Value::F64(value) => <f64 as WasmExternType>::from_native(*value).to_binary(),
                            value => return Err(RuntimeError::new(format!(
                                "value `{:?}` is not supported as a returned value of a host function for the moment; please use `dyn_call` or the new API",
                                value
                            ))),
                        })
                    })
                    .collect::<Result<_, _>>()?;

                // Convert `Vec<i128>` into a `WasmTypeList`.
                let rets: Rets = Rets::from_slice(rets.as_slice()).map_err(|_| {
                    RuntimeError::new(format!(
                        "returned values (`{:?}`) do not match the expected returned type (`{:?}`)",
                        results,
                        Rets::wasm_types()
                    ))
                })?;

                Ok(rets)
            }
        }
    }
}

func_call!();
func_call!(A1);
func_call!(A1, A2);
func_call!(A1, A2, A3);
func_call!(A1, A2, A3, A4);
func_call!(A1, A2, A3, A4, A5);
func_call!(A1, A2, A3, A4, A5, A6);
func_call!(A1, A2, A3, A4, A5, A6, A7);
func_call!(A1, A2, A3, A4, A5, A6, A7, A8);
func_call!(A1, A2, A3, A4, A5, A6, A7, A8, A9);
func_call!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10);
func_call!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11);
func_call!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12);
func_call!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13);
func_call!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14);
func_call!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15);
func_call!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16);
func_call!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17);
func_call!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18);
func_call!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19);
func_call!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20
);

impl<Args, Rets> From<Func<Args, Rets>> for new::wasmer::Extern
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    fn from(func: Func<Args, Rets>) -> Self {
        new::wasmer::Extern::Function(func.new_function)
    }
}

impl<Args, Rets> From<&new::wasmer::Function> for Func<Args, Rets>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    fn from(new_function: &new::wasmer::Function) -> Self {
        Self {
            new_function: new_function.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<'a, Args, Rets> new::wasmer::Exportable<'a> for Func<Args, Rets>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    fn to_export(&self) -> new::wasmer_runtime::Export {
        self.new_function.to_export()
    }

    fn get_self_from_extern(r#extern: &'a new::wasmer::Extern) -> Result<&'a Self, ExportError> {
        match r#extern {
            new::wasmer::Extern::Function(func) => Ok(
                // It's not ideal to call `Box::leak` here, but it
                // would introduce too much changes in the
                // `new::wasmer` API to support `Cow` or similar.
                Box::leak(Box::<Func<Args, Rets>>::new(func.into())),
            ),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}

/// Represents a type-erased function provided by either the host or the WebAssembly program.
#[derive(Clone)]
pub struct DynamicFunc {
    new_function: new::wasmer::Function,
}

use std::{
    cell::{RefCell, RefMut},
    ops::DerefMut,
    rc::Rc,
};

/// Specific context for `DynamicFunc`. It's a hack.
///
/// Initially, it holds an empty `vm::Ctx`, but it is replaced by the
/// `vm::Ctx` from `instance::PreInstance` in
/// `module::Module::instantiate`.
pub(crate) struct DynamicCtx {
    pub(crate) vmctx: Rc<RefCell<vm::Ctx>>,
}

impl DynamicFunc {
    /// Create a new `DynamicFunc`.
    pub fn new<F>(signature: &FuncSig, func: F) -> Self
    where
        F: Fn(&mut vm::Ctx, &[Value]) -> Result<Vec<Value>, RuntimeError> + 'static,
    {
        // Create an empty `vm::Ctx`, that is going to be overwritten by `Instance::new`.
        let ctx = DynamicCtx {
            vmctx: Rc::new(RefCell::new(unsafe { vm::Ctx::new_uninit() })),
        };

        Self {
            new_function: new::wasmer::Function::new_dynamic_env(
                get_global_store(),
                signature,
                ctx,
                // Wrapper to safely extract a `&mut vm::Ctx` to pass
                // to `func`.
                move |dyn_ctx: &mut DynamicCtx,
                      params: &[Value]|
                      -> Result<Vec<Value>, RuntimeError> {
                    let cell: Rc<RefCell<vm::Ctx>> = dyn_ctx.vmctx.clone();
                    let mut vmctx: RefMut<vm::Ctx> = cell.borrow_mut();

                    func(vmctx.deref_mut(), params)
                },
            ),
        }
    }

    /// Returns the full function signature.
    pub fn signature(&self) -> &FuncDescriptor {
        self.new_function.ty()
    }

    /// Returns the types of the function inputs.
    pub fn params(&self) -> &[Type] {
        self.signature().params()
    }

    /// Returns the types of the function outputs.
    pub fn returns(&self) -> &[Type] {
        self.signature().results()
    }

    /// Call the function. In this case, it's an alias to `dyn_call`.
    pub fn call(&self, params: &[Value]) -> Result<Box<[Value]>, RuntimeError> {
        self.dyn_call(params)
    }

    /// Call the function.
    pub fn dyn_call(&self, params: &[Value]) -> Result<Box<[Value]>, RuntimeError> {
        self.new_function.call(params)
    }
}

impl From<DynamicFunc> for new::wasmer::Extern {
    fn from(dynamic_func: DynamicFunc) -> Self {
        new::wasmer::Extern::Function(dynamic_func.new_function)
    }
}

impl From<&new::wasmer::Function> for DynamicFunc {
    fn from(new_function: &new::wasmer::Function) -> Self {
        Self {
            new_function: new_function.clone(),
        }
    }
}

impl<'a> new::wasmer::Exportable<'a> for DynamicFunc {
    fn to_export(&self) -> new::wasmer_runtime::Export {
        self.new_function.to_export()
    }

    fn get_self_from_extern(r#extern: &'a new::wasmer::Extern) -> Result<&'a Self, ExportError> {
        match r#extern {
            new::wasmer::Extern::Function(dynamic_func) => Ok(
                // It's not ideal to call `Box::leak` here, but it
                // would introduce too much changes in the
                // `new::wasmer` API to support `Cow` or similar.
                Box::leak(Box::<DynamicFunc>::new(dynamic_func.into())),
            ),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
