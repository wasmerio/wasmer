use crate::sys::context::{AsContextMut, AsContextRef, ContextInner, ContextMut};
use crate::sys::exports::{ExportError, Exportable};
use crate::sys::externals::Extern;
use crate::sys::FunctionType;
use crate::sys::NativeFunc;
use crate::sys::RuntimeError;

use crate::Value;
pub use inner::{FromToNativeWasmType, HostFunction, WasmTypeList};
use wasmer_types::RawValue;

use inner::StaticFunction;
use std::cell::UnsafeCell;
use std::cmp::max;
use std::ffi::c_void;
use wasmer_vm::{
    on_host_stack, raise_user_trap, resume_panic, wasmer_call_trampoline, ContextHandle,
    InternalContextHandle, MaybeInstanceOwned, VMCallerCheckedAnyfunc, VMContext,
    VMDynamicFunctionContext, VMExtern, VMFuncRef, VMFunction, VMFunctionBody,
    VMFunctionEnvironment, VMFunctionKind, VMTrampoline,
};

/// A WebAssembly `function` instance.
///
/// A function instance is the runtime representation of a function.
/// It effectively is a closure of the original function (defined in either
/// the host or the WebAssembly module) over the runtime `Instance` of its
/// originating `Module`.
///
/// The module instance is used to resolve references to other definitions
/// during execution of the function.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#function-instances>
///
/// # Panics
/// - Closures (functions with captured environments) are not currently supported
///   with native functions. Attempting to create a native `Function` with one will
///   result in a panic.
///   [Closures as host functions tracking issue](https://github.com/wasmerio/wasmer/issues/1840)
#[derive(Debug, Clone)]
pub struct Function {
    pub(crate) handle: ContextHandle<VMFunction>,
}

impl Function {
    /// Creates a new host `Function` (dynamic) with the provided signature.
    ///
    /// If you know the signature of the host function at compile time,
    /// consider using [`Function::new_native`] for less runtime overhead.
    ///
    /// # Examples
    ///
    /// ```
    /// # use wasmer::{Function, FunctionType, Type, Store, Value};
    /// # let store = Store::default();
    /// #
    /// let signature = FunctionType::new(vec![Type::I32, Type::I32], vec![Type::I32]);
    ///
    /// let f = Function::new(&store, &signature, |args| {
    ///     let sum = args[0].unwrap_i32() + args[1].unwrap_i32();
    ///     Ok(vec![Value::I32(sum)])
    /// });
    /// ```
    ///
    /// With constant signature:
    ///
    /// ```
    /// # use wasmer::{Function, FunctionType, Type, Store, Value};
    /// # let store = Store::default();
    /// #
    /// const I32_I32_TO_I32: ([Type; 2], [Type; 1]) = ([Type::I32, Type::I32], [Type::I32]);
    ///
    /// let f = Function::new(&store, I32_I32_TO_I32, |args| {
    ///     let sum = args[0].unwrap_i32() + args[1].unwrap_i32();
    ///     Ok(vec![Value::I32(sum)])
    /// });
    /// ```
    pub fn new<FT, F, T>(mut ctx: impl AsContextMut<Data = T>, ty: FT, func: F) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(ContextMut<'_, T>, &[Value]) -> Result<Vec<Value>, RuntimeError>
            + 'static
            + Send
            + Sync,
    {
        let mut ctx = ctx.as_context_mut();
        let function_type = ty.into();
        let func_ty = function_type.clone();
        let raw_ctx = ctx.as_raw() as *mut u8;
        let wrapper = move |values_vec: *mut RawValue| -> Result<(), RuntimeError> {
            unsafe {
                let mut ctx = ContextMut::from_raw(raw_ctx as *mut ContextInner<T>);
                let mut args = Vec::with_capacity(func_ty.params().len());
                for (i, ty) in func_ty.params().iter().enumerate() {
                    args.push(Value::from_raw(
                        ctx.as_context_mut(),
                        *ty,
                        *values_vec.add(i),
                    ));
                }
                let returns = func(ctx.as_context_mut(), &args)?;

                // We need to dynamically check that the returns
                // match the expected types, as well as expected length.
                let return_types = returns.iter().map(|ret| ret.ty());
                if return_types.ne(func_ty.results().iter().copied()) {
                    return Err(RuntimeError::new(format!(
                        "Dynamic function returned wrong signature. Expected {:?} but got {:?}",
                        func_ty.results(),
                        returns.iter().map(|ret| ret.ty())
                    )));
                }
                for (i, ret) in returns.iter().enumerate() {
                    *values_vec.add(i) = ret.as_raw(ctx.as_context_mut());
                }
            }
            Ok(())
        };
        let mut host_data = Box::new(VMDynamicFunctionContext {
            address: std::ptr::null(),
            ctx: DynamicFunction { func: wrapper },
        });
        host_data.address = host_data.ctx.func_body_ptr();

        // We don't yet have the address with the Wasm ABI signature.
        // The engine linker will replace the address with one pointing to a
        // generated dynamic trampoline.
        let func_ptr = std::ptr::null() as *const VMFunctionBody;
        let type_index = ctx.store().engine().register_signature(&function_type);
        let vmctx = VMFunctionEnvironment {
            host_env: host_data.as_ref() as *const _ as *mut c_void,
        };
        let call_trampoline = host_data.ctx.call_trampoline_address();
        let anyfunc = VMCallerCheckedAnyfunc {
            func_ptr,
            type_index,
            vmctx,
            call_trampoline,
        };

        let vm_function = VMFunction {
            anyfunc: MaybeInstanceOwned::Host(Box::new(UnsafeCell::new(anyfunc))),
            kind: VMFunctionKind::Dynamic,
            signature: function_type,
            host_data,
        };
        Self {
            handle: ContextHandle::new(ctx.objects_mut(), vm_function),
        }
    }

    /// Creates a new host `Function` from a native function.
    ///
    /// The function signature is automatically retrieved using the
    /// Rust typing system.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Store, Function};
    /// # let store = Store::default();
    /// #
    /// fn sum(a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
    /// let f = Function::new_native(&store, sum);
    /// ```
    pub fn new_native<T, F, Args, Rets>(mut ctx: impl AsContextMut<Data = T>, func: F) -> Self
    where
        F: HostFunction<T, Args, Rets> + 'static + Send + Sync,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        let mut ctx = ctx.as_context_mut();
        let host_data = Box::new(StaticFunction {
            raw_ctx: ctx.as_raw() as *mut u8,
            func,
        });
        let function_type = FunctionType::new(Args::wasm_types(), Rets::wasm_types());

        let func_ptr = <F as HostFunction<T, Args, Rets>>::function_body_ptr();
        let type_index = ctx.store().engine().register_signature(&function_type);
        let vmctx = VMFunctionEnvironment {
            host_env: host_data.as_ref() as *const _ as *mut c_void,
        };
        let call_trampoline = <F as HostFunction<T, Args, Rets>>::call_trampoline_address();
        let anyfunc = VMCallerCheckedAnyfunc {
            func_ptr,
            type_index,
            vmctx,
            call_trampoline,
        };

        let vm_function = VMFunction {
            anyfunc: MaybeInstanceOwned::Host(Box::new(UnsafeCell::new(anyfunc))),
            kind: VMFunctionKind::Static,
            signature: function_type,
            host_data,
        };
        Self {
            handle: ContextHandle::new(ctx.objects_mut(), vm_function),
        }
    }

    /// Returns the [`FunctionType`] of the `Function`.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Function, Store, Type};
    /// # let store = Store::default();
    /// #
    /// fn sum(a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
    /// let f = Function::new_native(&store, sum);
    ///
    /// assert_eq!(f.ty().params(), vec![Type::I32, Type::I32]);
    /// assert_eq!(f.ty().results(), vec![Type::I32]);
    /// ```
    pub fn ty(&self, ctx: impl AsContextRef) -> FunctionType {
        self.handle
            .get(ctx.as_context_ref().objects())
            .signature
            .clone()
    }

    fn call_wasm(
        &self,
        mut ctx: impl AsContextMut,
        trampoline: VMTrampoline,
        params: &[Value],
        results: &mut [Value],
    ) -> Result<(), RuntimeError> {
        let format_types_for_error_message = |items: &[Value]| {
            items
                .iter()
                .map(|param| param.ty().to_string())
                .collect::<Vec<String>>()
                .join(", ")
        };
        // TODO: Avoid cloning the signature here, it's expensive.
        let signature = self.ty(ctx.as_context_ref());
        if signature.params().len() != params.len() {
            return Err(RuntimeError::new(format!(
                "Parameters of type [{}] did not match signature {}",
                format_types_for_error_message(params),
                &signature
            )));
        }
        if signature.results().len() != results.len() {
            return Err(RuntimeError::new(format!(
                "Results of type [{}] did not match signature {}",
                format_types_for_error_message(results),
                &signature,
            )));
        }

        let mut values_vec = vec![RawValue { i32: 0 }; max(params.len(), results.len())];

        // Store the argument values into `values_vec`.
        let param_tys = signature.params().iter();
        for ((arg, slot), ty) in params.iter().zip(&mut values_vec).zip(param_tys) {
            if arg.ty() != *ty {
                let param_types = format_types_for_error_message(params);
                return Err(RuntimeError::new(format!(
                    "Parameters of type [{}] did not match signature {}",
                    param_types, &signature,
                )));
            }
            *slot = arg.as_raw(ctx.as_context_mut());
        }

        // Call the trampoline.
        let vm_function = self.handle.get(ctx.as_context_ref().objects());
        if let Err(error) = unsafe {
            wasmer_call_trampoline(
                ctx.as_context_ref().store(),
                vm_function.anyfunc.as_ptr().as_ref().vmctx,
                trampoline,
                vm_function.anyfunc.as_ptr().as_ref().func_ptr,
                values_vec.as_mut_ptr() as *mut u8,
            )
        } {
            return Err(RuntimeError::from_trap(error));
        }

        // Load the return values out of `values_vec`.
        for (index, &value_type) in signature.results().iter().enumerate() {
            unsafe {
                results[index] =
                    Value::from_raw(ctx.as_context_mut(), value_type, values_vec[index]);
            }
        }

        Ok(())
    }

    /// Returns the number of parameters that this function takes.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Function, Store, Type};
    /// # let store = Store::default();
    /// #
    /// fn sum(a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
    /// let f = Function::new_native(&store, sum);
    ///
    /// assert_eq!(f.param_arity(), 2);
    /// ```
    pub fn param_arity(&self, ctx: impl AsContextRef) -> usize {
        self.ty(ctx).params().len()
    }

    /// Returns the number of results this function produces.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Function, Store, Type};
    /// # let store = Store::default();
    /// #
    /// fn sum(a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
    /// let f = Function::new_native(&store, sum);
    ///
    /// assert_eq!(f.result_arity(), 1);
    /// ```
    pub fn result_arity(&self, ctx: impl AsContextRef) -> usize {
        self.ty(ctx).results().len()
    }

    /// Call the `Function` function.
    ///
    /// Depending on where the Function is defined, it will call it.
    /// 1. If the function is defined inside a WebAssembly, it will call the trampoline
    ///    for the function signature.
    /// 2. If the function is defined in the host (in a native way), it will
    ///    call the trampoline.
    ///
    /// # Examples
    ///
    /// ```
    /// # use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, Type, Value};
    /// # let store = Store::default();
    /// # let wasm_bytes = wat2wasm(r#"
    /// # (module
    /// #   (func (export "sum") (param $x i32) (param $y i32) (result i32)
    /// #     local.get $x
    /// #     local.get $y
    /// #     i32.add
    /// #   ))
    /// # "#.as_bytes()).unwrap();
    /// # let module = Module::new(&store, wasm_bytes).unwrap();
    /// # let import_object = imports! {};
    /// # let instance = Instance::new(&module, &import_object).unwrap();
    /// #
    /// let sum = instance.exports.get_function("sum").unwrap();
    ///
    /// assert_eq!(sum.call(&[Value::I32(1), Value::I32(2)]).unwrap().to_vec(), vec![Value::I32(3)]);
    /// ```
    pub fn call(
        &self,
        mut ctx: impl AsContextMut,
        params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        let trampoline = unsafe {
            self.handle
                .get(ctx.as_context_ref().objects())
                .anyfunc
                .as_ptr()
                .as_ref()
                .call_trampoline
        };
        let mut results = vec![Value::null(); self.result_arity(ctx.as_context_ref())];
        self.call_wasm(ctx.as_context_mut(), trampoline, params, &mut results)?;
        return Ok(results.into_boxed_slice());
    }

    pub(crate) fn vm_funcref(&self, ctx: impl AsContextRef) -> VMFuncRef {
        let vm_function = self.handle.get(ctx.as_context_ref().objects());
        if vm_function.kind == VMFunctionKind::Dynamic {
            panic!("dynamic functions cannot be used in tables or as funcrefs");
        }
        VMFuncRef(vm_function.anyfunc.as_ptr())
    }

    pub(crate) unsafe fn from_vm_funcref(mut ctx: impl AsContextMut, funcref: VMFuncRef) -> Self {
        let signature = ctx
            .as_context_mut()
            .store()
            .engine()
            .lookup_signature(funcref.0.as_ref().type_index)
            .expect("Signature not found in store");
        let vm_function = VMFunction {
            anyfunc: MaybeInstanceOwned::Instance(funcref.0),
            signature,
            // All functions in tables are already Static (as dynamic functions
            // are converted to use the trampolines with static signatures).
            kind: wasmer_vm::VMFunctionKind::Static,
            host_data: Box::new(()),
        };
        Self {
            handle: ContextHandle::new(ctx.as_context_mut().objects_mut(), vm_function),
        }
    }

    /// Transform this WebAssembly function into a function with the
    /// native ABI. See [`NativeFunc`] to learn more.
    ///
    /// # Examples
    ///
    /// ```
    /// # use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, Type, Value};
    /// # let store = Store::default();
    /// # let wasm_bytes = wat2wasm(r#"
    /// # (module
    /// #   (func (export "sum") (param $x i32) (param $y i32) (result i32)
    /// #     local.get $x
    /// #     local.get $y
    /// #     i32.add
    /// #   ))
    /// # "#.as_bytes()).unwrap();
    /// # let module = Module::new(&store, wasm_bytes).unwrap();
    /// # let import_object = imports! {};
    /// # let instance = Instance::new(&module, &import_object).unwrap();
    /// #
    /// let sum = instance.exports.get_function("sum").unwrap();
    /// let sum_native = sum.native::<(i32, i32), i32>().unwrap();
    ///
    /// assert_eq!(sum_native.call(1, 2).unwrap(), 3);
    /// ```
    ///
    /// # Errors
    ///
    /// If the `Args` generic parameter does not match the exported function
    /// an error will be raised:
    ///
    /// ```should_panic
    /// # use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, Type, Value};
    /// # let store = Store::default();
    /// # let wasm_bytes = wat2wasm(r#"
    /// # (module
    /// #   (func (export "sum") (param $x i32) (param $y i32) (result i32)
    /// #     local.get $x
    /// #     local.get $y
    /// #     i32.add
    /// #   ))
    /// # "#.as_bytes()).unwrap();
    /// # let module = Module::new(&store, wasm_bytes).unwrap();
    /// # let import_object = imports! {};
    /// # let instance = Instance::new(&module, &import_object).unwrap();
    /// #
    /// let sum = instance.exports.get_function("sum").unwrap();
    ///
    /// // This results in an error: `RuntimeError`
    /// let sum_native = sum.native::<(i64, i64), i32>().unwrap();
    /// ```
    ///
    /// If the `Rets` generic parameter does not match the exported function
    /// an error will be raised:
    ///
    /// ```should_panic
    /// # use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, Type, Value};
    /// # let store = Store::default();
    /// # let wasm_bytes = wat2wasm(r#"
    /// # (module
    /// #   (func (export "sum") (param $x i32) (param $y i32) (result i32)
    /// #     local.get $x
    /// #     local.get $y
    /// #     i32.add
    /// #   ))
    /// # "#.as_bytes()).unwrap();
    /// # let module = Module::new(&store, wasm_bytes).unwrap();
    /// # let import_object = imports! {};
    /// # let instance = Instance::new(&module, &import_object).unwrap();
    /// #
    /// let sum = instance.exports.get_function("sum").unwrap();
    ///
    /// // This results in an error: `RuntimeError`
    /// let sum_native = sum.native::<(i32, i32), i64>().unwrap();
    /// ```
    pub fn native<Args, Rets>(
        &self,
        ctx: impl AsContextRef,
    ) -> Result<NativeFunc<Args, Rets>, RuntimeError>
    where
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        let vm_function = self.handle.get(ctx.as_context_ref().objects());

        // type check
        {
            let expected = vm_function.signature.params();
            let given = Args::wasm_types();

            if expected != given {
                return Err(RuntimeError::new(format!(
                    "given types (`{:?}`) for the function arguments don't match the actual types (`{:?}`)",
                    given,
                    expected,
                )));
            }
        }

        {
            let expected = vm_function.signature.results();
            let given = Rets::wasm_types();

            if expected != given {
                // todo: error result types don't match
                return Err(RuntimeError::new(format!(
                    "given types (`{:?}`) for the function results don't match the actual types (`{:?}`)",
                    given,
                    expected,
                )));
            }
        }

        Ok(NativeFunc::new(self.clone()))
    }

    pub(crate) fn from_vm_extern(
        ctx: impl AsContextMut,
        internal: InternalContextHandle<VMFunction>,
    ) -> Self {
        Self {
            handle: unsafe {
                ContextHandle::from_internal(ctx.as_context_ref().objects().id(), internal)
            },
        }
    }

    /// Checks whether this `Function` can be used with the given context.
    pub fn is_from_context(&self, ctx: impl AsContextRef) -> bool {
        self.handle.context_id() == ctx.as_context_ref().objects().id()
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Function(self.handle.internal_handle())
    }
}

impl<'a> Exportable<'a> for Function {
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Function(func) => Ok(func),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}

/// Host state for a dynamic function.
pub(crate) struct DynamicFunction<F> {
    func: F,
}

impl<F> DynamicFunction<F>
where
    F: Fn(*mut RawValue) -> Result<(), RuntimeError> + 'static,
{
    // This function wraps our func, to make it compatible with the
    // reverse trampoline signature
    unsafe extern "C" fn func_wrapper(&self, values_vec: *mut RawValue) {
        use std::panic::{self, AssertUnwindSafe};
        let result =
            on_host_stack(|| panic::catch_unwind(AssertUnwindSafe(|| (self.func)(values_vec))));

        match result {
            Ok(Ok(())) => {}
            Ok(Err(trap)) => raise_user_trap(Box::new(trap)),
            Err(panic) => resume_panic(panic),
        }
    }

    fn func_body_ptr(&self) -> *const VMFunctionBody {
        Self::func_wrapper as *const VMFunctionBody
    }

    fn call_trampoline_address(&self) -> VMTrampoline {
        Self::call_trampoline
    }

    unsafe extern "C" fn call_trampoline(
        vmctx: *mut VMContext,
        _body: *const VMFunctionBody,
        args: *mut RawValue,
    ) {
        // The VMFunctionBody pointer is null here: it is only filled in later
        // by the engine linker.
        let dynamic_function = &mut *(vmctx as *mut Self);
        Self::func_wrapper(dynamic_function, args);
    }
}

/// This private inner module contains the low-level implementation
/// for `Function` and its siblings.
mod inner {
    use std::array::TryFromSliceError;
    use std::convert::{Infallible, TryInto};
    use std::error::Error;
    use std::panic::{self, AssertUnwindSafe};
    use wasmer_vm::{on_host_stack, VMTrampoline};

    use wasmer_types::{RawValue, Type};
    use wasmer_vm::{raise_user_trap, resume_panic, VMFunctionBody};

    use crate::sys::context::{ContextInner, ContextMut};
    use crate::sys::NativeWasmType;
    use crate::{AsContextMut, ExternRef, Function};

    /// A trait to convert a Rust value to a `WasmNativeType` value,
    /// or to convert `WasmNativeType` value to a Rust value.
    ///
    /// This trait should ideally be split into two traits:
    /// `FromNativeWasmType` and `ToNativeWasmType` but it creates a
    /// non-negligible complexity in the `WasmTypeList`
    /// implementation.
    pub unsafe trait FromToNativeWasmType
    where
        Self: Sized,
    {
        /// Native Wasm type.
        type Native: NativeWasmType;

        /// Convert a value of kind `Self::Native` to `Self`.
        ///
        /// # Panics
        ///
        /// This method panics if `native` cannot fit in the `Self`
        /// type`.
        fn from_native(native: Self::Native) -> Self;

        /// Convert self to `Self::Native`.
        ///
        /// # Panics
        ///
        /// This method panics if `self` cannot fit in the
        /// `Self::Native` type.
        fn to_native(self) -> Self::Native;
    }

    macro_rules! from_to_native_wasm_type {
        ( $( $type:ty => $native_type:ty ),* ) => {
            $(
                #[allow(clippy::use_self)]
                unsafe impl FromToNativeWasmType for $type {
                    type Native = $native_type;

                    #[inline]
                    fn from_native(native: Self::Native) -> Self {
                        native as Self
                    }

                    #[inline]
                    fn to_native(self) -> Self::Native {
                        self as Self::Native
                    }
                }
            )*
        };
    }

    macro_rules! from_to_native_wasm_type_same_size {
        ( $( $type:ty => $native_type:ty ),* ) => {
            $(
                #[allow(clippy::use_self)]
                unsafe impl FromToNativeWasmType for $type {
                    type Native = $native_type;

                    #[inline]
                    fn from_native(native: Self::Native) -> Self {
                        Self::from_ne_bytes(Self::Native::to_ne_bytes(native))
                    }

                    #[inline]
                    fn to_native(self) -> Self::Native {
                        Self::Native::from_ne_bytes(Self::to_ne_bytes(self))
                    }
                }
            )*
        };
    }

    from_to_native_wasm_type!(
        i8 => i32,
        u8 => i32,
        i16 => i32,
        u16 => i32
    );

    from_to_native_wasm_type_same_size!(
        i32 => i32,
        u32 => i32,
        i64 => i64,
        u64 => i64,
        f32 => f32,
        f64 => f64
    );

    unsafe impl FromToNativeWasmType for Option<ExternRef> {
        type Native = Option<ExternRef>;

        fn to_native(self) -> Self::Native {
            self
        }
        fn from_native(n: Self::Native) -> Self {
            n
        }
    }

    unsafe impl FromToNativeWasmType for Option<Function> {
        type Native = Option<Function>;

        fn to_native(self) -> Self::Native {
            self
        }
        fn from_native(n: Self::Native) -> Self {
            n
        }
    }

    #[cfg(test)]
    mod test_from_to_native_wasm_type {
        use super::*;

        #[test]
        fn test_to_native() {
            assert_eq!(7i8.to_native(), 7i32);
            assert_eq!(7u8.to_native(), 7i32);
            assert_eq!(7i16.to_native(), 7i32);
            assert_eq!(7u16.to_native(), 7i32);
            assert_eq!(u32::MAX.to_native(), -1);
        }

        #[test]
        fn test_to_native_same_size() {
            assert_eq!(7i32.to_native(), 7i32);
            assert_eq!(7u32.to_native(), 7i32);
            assert_eq!(7i64.to_native(), 7i64);
            assert_eq!(7u64.to_native(), 7i64);
            assert_eq!(7f32.to_native(), 7f32);
            assert_eq!(7f64.to_native(), 7f64);
        }
    }

    /// The `WasmTypeList` trait represents a tuple (list) of Wasm
    /// typed values. It is used to get low-level representation of
    /// such a tuple.
    pub trait WasmTypeList
    where
        Self: Sized,
    {
        /// The C type (a struct) that can hold/represent all the
        /// represented values.
        type CStruct;

        /// The array type that can hold all the represented values.
        ///
        /// Note that all values are stored in their binary form.
        type Array: AsMut<[RawValue]>;

        /// Constructs `Self` based on an array of values.
        unsafe fn from_array(ctx: impl AsContextMut, array: Self::Array) -> Self;

        /// Constructs `Self` based on a slice of values.
        ///
        /// `from_slice` returns a `Result` because it is possible
        /// that the slice doesn't have the same size than
        /// `Self::Array`, in which circumstance an error of kind
        /// `TryFromSliceError` will be returned.
        unsafe fn from_slice(
            ctx: impl AsContextMut,
            slice: &[RawValue],
        ) -> Result<Self, TryFromSliceError>;

        /// Builds and returns an array of type `Array` from a tuple
        /// (list) of values.
        unsafe fn into_array(self, ctx: impl AsContextMut) -> Self::Array;

        /// Allocates and return an empty array of type `Array` that
        /// will hold a tuple (list) of values, usually to hold the
        /// returned values of a WebAssembly function call.
        fn empty_array() -> Self::Array;

        /// Builds a tuple (list) of values from a C struct of type
        /// `CStruct`.
        unsafe fn from_c_struct(ctx: impl AsContextMut, c_struct: Self::CStruct) -> Self;

        /// Builds and returns a C struct of type `CStruct` from a
        /// tuple (list) of values.
        unsafe fn into_c_struct(self, ctx: impl AsContextMut) -> Self::CStruct;

        /// Get the Wasm types for the tuple (list) of currently
        /// represented values.
        fn wasm_types() -> &'static [Type];
    }

    /// The `IntoResult` trait turns a `WasmTypeList` into a
    /// `Result<WasmTypeList, Self::Error>`.
    ///
    /// It is mostly used to turn result values of a Wasm function
    /// call into a `Result`.
    pub trait IntoResult<T>
    where
        T: WasmTypeList,
    {
        /// The error type for this trait.
        type Error: Error + Sync + Send + 'static;

        /// Transforms `Self` into a `Result`.
        fn into_result(self) -> Result<T, Self::Error>;
    }

    impl<T> IntoResult<T> for T
    where
        T: WasmTypeList,
    {
        // `T` is not a `Result`, it's already a value, so no error
        // can be built.
        type Error = Infallible;

        fn into_result(self) -> Result<Self, Infallible> {
            Ok(self)
        }
    }

    impl<T, E> IntoResult<T> for Result<T, E>
    where
        T: WasmTypeList,
        E: Error + Sync + Send + 'static,
    {
        type Error = E;

        fn into_result(self) -> Self {
            self
        }
    }

    #[cfg(test)]
    mod test_into_result {
        use super::*;
        use std::convert::Infallible;

        #[test]
        fn test_into_result_over_t() {
            let x: i32 = 42;
            let result_of_x: Result<i32, Infallible> = x.into_result();

            assert_eq!(result_of_x.unwrap(), x);
        }

        #[test]
        fn test_into_result_over_result() {
            {
                let x: Result<i32, Infallible> = Ok(42);
                let result_of_x: Result<i32, Infallible> = x.into_result();

                assert_eq!(result_of_x, x);
            }

            {
                use std::{error, fmt};

                #[derive(Debug, PartialEq)]
                struct E;

                impl fmt::Display for E {
                    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                        write!(formatter, "E")
                    }
                }

                impl error::Error for E {}

                let x: Result<Infallible, E> = Err(E);
                let result_of_x: Result<Infallible, E> = x.into_result();

                assert_eq!(result_of_x.unwrap_err(), E);
            }
        }
    }

    /// The `HostFunction` trait represents the set of functions that
    /// can be used as host function. To uphold this statement, it is
    /// necessary for a function to be transformed into a pointer to
    /// `VMFunctionBody`.
    pub trait HostFunction<T, Args, Rets>
    where
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        /// Get the pointer to the function body.
        fn function_body_ptr() -> *const VMFunctionBody;

        /// Get the pointer to the function call trampoline.
        fn call_trampoline_address() -> VMTrampoline;
    }

    /// Represents a low-level Wasm static host function. See
    /// `super::Function::new_native` to learn more.
    pub(crate) struct StaticFunction<F> {
        pub(crate) raw_ctx: *mut u8,
        pub(crate) func: F,
    }

    macro_rules! impl_host_function {
        ( [$c_struct_representation:ident]
           $c_struct_name:ident,
           $( $x:ident ),* ) => {

            /// A structure with a C-compatible representation that can hold a set of Wasm values.
            /// This type is used by `WasmTypeList::CStruct`.
            #[repr($c_struct_representation)]
            pub struct $c_struct_name< $( $x ),* > ( $( <<$x as FromToNativeWasmType>::Native as NativeWasmType>::Abi ),* )
            where
                $( $x: FromToNativeWasmType ),*;

            // Implement `WasmTypeList` for a specific tuple.
            #[allow(unused_parens, dead_code)]
            impl< $( $x ),* >
                WasmTypeList
            for
                ( $( $x ),* )
            where
                $( $x: FromToNativeWasmType ),*
            {
                type CStruct = $c_struct_name< $( $x ),* >;

                type Array = [RawValue; count_idents!( $( $x ),* )];

                #[allow(unused_mut)]
                unsafe fn from_array(mut _ctx: impl AsContextMut, array: Self::Array) -> Self {
                    // Unpack items of the array.
                    #[allow(non_snake_case)]
                    let [ $( $x ),* ] = array;

                    // Build the tuple.
                    (
                        $(
                            FromToNativeWasmType::from_native(NativeWasmType::from_raw(_ctx.as_context_mut(), $x))
                        ),*
                    )
                }

                unsafe fn from_slice(ctx: impl AsContextMut, slice: &[RawValue]) -> Result<Self, TryFromSliceError> {
                    Ok(Self::from_array(ctx, slice.try_into()?))
                }

                #[allow(unused_mut)]
                unsafe fn into_array(self, mut _ctx: impl AsContextMut) -> Self::Array {
                    // Unpack items of the tuple.
                    #[allow(non_snake_case)]
                    let ( $( $x ),* ) = self;

                    // Build the array.
                    [
                        $(
                            FromToNativeWasmType::to_native($x).into_raw(_ctx.as_context_mut())
                        ),*
                    ]
                }

                fn empty_array() -> Self::Array {
                    // Build an array initialized with `0`.
                    [RawValue { i32: 0 }; count_idents!( $( $x ),* )]
                }

                #[allow(unused_mut)]
                unsafe fn from_c_struct(mut _ctx: impl AsContextMut, c_struct: Self::CStruct) -> Self {
                    // Unpack items of the C structure.
                    #[allow(non_snake_case)]
                    let $c_struct_name( $( $x ),* ) = c_struct;

                    (
                        $(
                            FromToNativeWasmType::from_native(NativeWasmType::from_abi(_ctx.as_context_mut(), $x))
                        ),*
                    )
                }

                #[allow(unused_parens, non_snake_case, unused_mut)]
                unsafe fn into_c_struct(self, mut _ctx: impl AsContextMut) -> Self::CStruct {
                    // Unpack items of the tuple.
                    let ( $( $x ),* ) = self;

                    // Build the C structure.
                    $c_struct_name(
                        $(
                            FromToNativeWasmType::to_native($x).into_abi(_ctx.as_context_mut())
                        ),*
                    )
                }

                fn wasm_types() -> &'static [Type] {
                    &[
                        $(
                            $x::Native::WASM_TYPE
                        ),*
                    ]
                }
            }

            // Implement `HostFunction` for a function that has the same arity than the tuple.
            #[allow(unused_parens)]
            impl< $( $x, )* Rets, RetsAsResult, T, Func >
                HostFunction<T, ( $( $x ),* ), Rets>
            for
                Func
            where
                $( $x: FromToNativeWasmType, )*
                Rets: WasmTypeList,
                RetsAsResult: IntoResult<Rets>,
                Func: Fn(ContextMut<'_, T>, $( $x , )*) -> RetsAsResult + 'static,
            {
                #[allow(non_snake_case)]
                fn function_body_ptr() -> *const VMFunctionBody {
                    /// This is a function that wraps the real host
                    /// function. Its address will be used inside the
                    /// runtime.
                    unsafe extern "C" fn func_wrapper<T, $( $x, )* Rets, RetsAsResult, Func>( env: &StaticFunction<Func>, $( $x: $x::Native, )* ) -> Rets::CStruct
                    where
                        $( $x: FromToNativeWasmType, )*
                        Rets: WasmTypeList,
                        RetsAsResult: IntoResult<Rets>,
                        Func: Fn(ContextMut<'_, T>, $( $x , )*) -> RetsAsResult + 'static,
                    {
                        let mut ctx = ContextMut::from_raw(env.raw_ctx as *mut ContextInner<T>);

                        let result = on_host_stack(|| {
                            panic::catch_unwind(AssertUnwindSafe(|| {
                                (env.func)(ctx.as_context_mut(), $( FromToNativeWasmType::from_native($x) ),* ).into_result()
                            }))
                        });

                        match result {
                            Ok(Ok(result)) => return result.into_c_struct(ctx),
                            Ok(Err(trap)) => raise_user_trap(Box::new(trap)),
                            Err(panic) => resume_panic(panic) ,
                        }
                    }

                    func_wrapper::< T, $( $x, )* Rets, RetsAsResult, Self > as *const VMFunctionBody
                }

                fn call_trampoline_address() -> VMTrampoline {
                    todo!()
                }
            }
        };
    }

    // Black-magic to count the number of identifiers at compile-time.
    macro_rules! count_idents {
        ( $($idents:ident),* ) => {
            {
                #[allow(dead_code, non_camel_case_types)]
                enum Idents { $( $idents, )* __CountIdentsLast }
                const COUNT: usize = Idents::__CountIdentsLast as usize;
                COUNT
            }
        };
    }

    // Here we go! Let's generate all the C struct, `WasmTypeList`
    // implementations and `HostFunction` implementations.
    impl_host_function!([C] S0,);
    impl_host_function!([transparent] S1, A1);
    impl_host_function!([C] S2, A1, A2);
    impl_host_function!([C] S3, A1, A2, A3);
    impl_host_function!([C] S4, A1, A2, A3, A4);
    impl_host_function!([C] S5, A1, A2, A3, A4, A5);
    impl_host_function!([C] S6, A1, A2, A3, A4, A5, A6);
    impl_host_function!([C] S7, A1, A2, A3, A4, A5, A6, A7);
    impl_host_function!([C] S8, A1, A2, A3, A4, A5, A6, A7, A8);
    impl_host_function!([C] S9, A1, A2, A3, A4, A5, A6, A7, A8, A9);
    impl_host_function!([C] S10, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10);
    impl_host_function!([C] S11, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11);
    impl_host_function!([C] S12, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12);
    impl_host_function!([C] S13, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13);
    impl_host_function!([C] S14, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14);
    impl_host_function!([C] S15, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15);
    impl_host_function!([C] S16, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16);
    impl_host_function!([C] S17, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17);
    impl_host_function!([C] S18, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18);
    impl_host_function!([C] S19, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19);
    impl_host_function!([C] S20, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20);
    impl_host_function!([C] S21, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21);
    impl_host_function!([C] S22, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22);
    impl_host_function!([C] S23, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23);
    impl_host_function!([C] S24, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24);
    impl_host_function!([C] S25, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24, A25);
    impl_host_function!([C] S26, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24, A25, A26);

    // Implement `WasmTypeList` on `Infallible`, which means that
    // `Infallible` can be used as a returned type of a host function
    // to express that it doesn't return, or to express that it cannot
    // fail (with `Result<_, Infallible>`).
    impl WasmTypeList for Infallible {
        type CStruct = Self;
        type Array = [RawValue; 0];

        unsafe fn from_array(_: impl AsContextMut, _: Self::Array) -> Self {
            unreachable!()
        }

        unsafe fn from_slice(
            _: impl AsContextMut,
            _: &[RawValue],
        ) -> Result<Self, TryFromSliceError> {
            unreachable!()
        }

        unsafe fn into_array(self, _: impl AsContextMut) -> Self::Array {
            []
        }

        fn empty_array() -> Self::Array {
            []
        }

        unsafe fn from_c_struct(_: impl AsContextMut, self_: Self::CStruct) -> Self {
            self_
        }

        unsafe fn into_c_struct(self, _: impl AsContextMut) -> Self::CStruct {
            self
        }

        fn wasm_types() -> &'static [Type] {
            &[]
        }
    }

    #[cfg(test)]
    mod test_wasm_type_list {
        use super::*;
        use wasmer_types::Type;

        #[test]
        fn test_from_array() {
            assert_eq!(<()>::from_array([]), ());
            assert_eq!(<i32>::from_array([1]), (1i32));
            assert_eq!(<(i32, i64)>::from_array([1, 2]), (1i32, 2i64));
            assert_eq!(
                <(i32, i64, f32, f64)>::from_array([
                    1,
                    2,
                    (3.1f32).to_bits().into(),
                    (4.2f64).to_bits().into()
                ]),
                (1, 2, 3.1f32, 4.2f64)
            );
        }

        #[test]
        fn test_into_array() {
            assert_eq!(().into_array(), [0i128; 0]);
            assert_eq!((1).into_array(), [1]);
            assert_eq!((1i32, 2i64).into_array(), [1, 2]);
            assert_eq!(
                (1i32, 2i32, 3.1f32, 4.2f64).into_array(),
                [1, 2, (3.1f32).to_bits().into(), (4.2f64).to_bits().into()]
            );
        }

        #[test]
        fn test_empty_array() {
            assert_eq!(<()>::empty_array().len(), 0);
            assert_eq!(<i32>::empty_array().len(), 1);
            assert_eq!(<(i32, i64)>::empty_array().len(), 2);
        }

        #[test]
        fn test_from_c_struct() {
            assert_eq!(<()>::from_c_struct(S0()), ());
            assert_eq!(<i32>::from_c_struct(S1(1)), (1i32));
            assert_eq!(<(i32, i64)>::from_c_struct(S2(1, 2)), (1i32, 2i64));
            assert_eq!(
                <(i32, i64, f32, f64)>::from_c_struct(S4(1, 2, 3.1, 4.2)),
                (1i32, 2i64, 3.1f32, 4.2f64)
            );
        }

        #[test]
        fn test_wasm_types_for_uni_values() {
            assert_eq!(<i32>::wasm_types(), [Type::I32]);
            assert_eq!(<i64>::wasm_types(), [Type::I64]);
            assert_eq!(<f32>::wasm_types(), [Type::F32]);
            assert_eq!(<f64>::wasm_types(), [Type::F64]);
        }

        #[test]
        fn test_wasm_types_for_multi_values() {
            assert_eq!(<(i32, i32)>::wasm_types(), [Type::I32, Type::I32]);
            assert_eq!(<(i64, i64)>::wasm_types(), [Type::I64, Type::I64]);
            assert_eq!(<(f32, f32)>::wasm_types(), [Type::F32, Type::F32]);
            assert_eq!(<(f64, f64)>::wasm_types(), [Type::F64, Type::F64]);

            assert_eq!(
                <(i32, i64, f32, f64)>::wasm_types(),
                [Type::I32, Type::I64, Type::F32, Type::F64]
            );
        }
    }

    #[allow(non_snake_case)]
    #[cfg(test)]
    mod test_function {
        use super::*;
        use wasmer_types::Type;

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
            assert_eq!(
                StaticFunction::new(func).ty(),
                FunctionType::new(vec![], vec![])
            );
            assert_eq!(
                StaticFunction::new(func__i32).ty(),
                FunctionType::new(vec![], vec![Type::I32])
            );
            assert_eq!(
                StaticFunction::new(func_i32).ty(),
                FunctionType::new(vec![Type::I32], vec![])
            );
            assert_eq!(
                StaticFunction::new(func_i32__i32).ty(),
                FunctionType::new(vec![Type::I32], vec![Type::I32])
            );
            assert_eq!(
                StaticFunction::new(func_i32_i32__i32).ty(),
                FunctionType::new(vec![Type::I32, Type::I32], vec![Type::I32])
            );
            assert_eq!(
                StaticFunction::new(func_i32_i32__i32_i32).ty(),
                FunctionType::new(vec![Type::I32, Type::I32], vec![Type::I32, Type::I32])
            );
            assert_eq!(
                StaticFunction::new(func_f32_i32__i32_f32).ty(),
                FunctionType::new(vec![Type::F32, Type::I32], vec![Type::I32, Type::F32])
            );
        }

        #[test]
        fn test_function_pointer() {
            let f = StaticFunction::new(func_i32__i32);
            let function = unsafe { std::mem::transmute::<_, fn(usize, i32) -> i32>(f.address) };
            assert_eq!(function(0, 3), 6);
        }
    }
}
