//! Data types, functions and traits for `sys` runtime's `Function` implementation.

pub(crate) mod env;
pub(crate) mod typed;

use crate::{
    BackendFunction, FunctionEnv, FunctionEnvMut, FunctionType, HostFunction, RuntimeError,
    StoreInner, Value, WithEnv, WithoutEnv,
    backend::sys::{engine::NativeEngineExt, vm::VMFunctionCallback},
    entities::store::{AsStoreMut, AsStoreRef, StoreMut},
    sys::async_runtime::AsyncRuntimeError,
    utils::{FromToNativeWasmType, IntoResult, NativeWasmTypeInto, WasmTypeList},
    vm::{VMExtern, VMExternFunction},
};
use std::panic::{self, AssertUnwindSafe};
use std::{
    cell::UnsafeCell, cmp::max, error::Error, ffi::c_void, future::Future, pin::Pin, sync::Arc,
};
use wasmer_types::{NativeWasmType, RawValue};
use wasmer_vm::{
    MaybeInstanceOwned, StoreHandle, Trap, TrapCode, VMCallerCheckedAnyfunc, VMContext,
    VMDynamicFunctionContext, VMFuncRef, VMFunction, VMFunctionBody, VMFunctionContext,
    VMFunctionKind, VMTrampoline, on_host_stack, raise_lib_trap, raise_user_trap, resume_panic,
    wasmer_call_trampoline,
};

use crate::backend::sys::async_runtime::{block_on_host_future, call_function_async};

#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[derive(Debug, Clone, PartialEq, Eq)]
/// A WebAssembly `function` instance, in the `sys` runtime.
pub struct Function {
    pub(crate) handle: StoreHandle<VMFunction>,
}

impl From<StoreHandle<VMFunction>> for Function {
    fn from(handle: StoreHandle<VMFunction>) -> Self {
        Self { handle }
    }
}

impl Function {
    pub(crate) fn new_with_env<FT, F, T: Send + 'static>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        ty: FT,
        func: F,
    ) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(FunctionEnvMut<T>, &[Value]) -> Result<Vec<Value>, RuntimeError>
            + 'static
            + Send
            + Sync,
    {
        let function_type = ty.into();
        let func_ty = function_type.clone();
        let func_env = env.clone().into_sys();
        let raw_store = store.as_store_mut().as_raw() as *mut StoreInner;
        let wrapper = move |values_vec: *mut RawValue| -> HostCallOutcome {
            unsafe {
                let mut store = StoreMut::from_raw(raw_store);
                let mut args = Vec::with_capacity(func_ty.params().len());

                for (i, ty) in func_ty.params().iter().enumerate() {
                    args.push(Value::from_raw(
                        &mut store,
                        *ty,
                        values_vec.add(i).read_unaligned(),
                    ));
                }
                let store_mut = StoreMut::from_raw(raw_store);
                let env = env::FunctionEnvMut {
                    store_mut,
                    func_env: func_env.clone(),
                }
                .into();
                let sig = func_ty.clone();
                let result = func(env, &args);
                HostCallOutcome::Ready {
                    func_ty: sig,
                    result,
                }
            }
        };
        let mut host_data = Box::new(VMDynamicFunctionContext {
            address: std::ptr::null(),
            ctx: DynamicFunction {
                func: wrapper,
                raw_store,
            },
        });
        host_data.address = host_data.ctx.func_body_ptr() as *const VMFunctionBody;

        // We don't yet have the address with the Wasm ABI signature.
        // The engine linker will replace the address with one pointing to a
        // generated dynamic trampoline.
        let func_ptr = std::ptr::null() as VMFunctionCallback;
        let type_index = store
            .as_store_mut()
            .engine()
            .as_sys()
            .register_signature(&function_type);
        let vmctx = VMFunctionContext {
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
            handle: StoreHandle::new(store.as_store_mut().objects_mut().as_sys_mut(), vm_function),
        }
    }

    pub(crate) fn new_async<FT, F, Fut>(store: &mut impl AsStoreMut, ty: FT, func: F) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(&[Value]) -> Fut + 'static + Send + Sync,
        Fut: Future<Output = Result<Vec<Value>, RuntimeError>> + 'static + Send,
    {
        let env = FunctionEnv::new(&mut store.as_store_mut(), ());
        let wrapped = move |_env: FunctionEnvMut<()>, values: &[Value]| func(values);
        Self::new_with_env_async(store, &env, ty, wrapped)
    }

    pub(crate) fn new_with_env_async<FT, F, Fut, T: Send + 'static>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        ty: FT,
        func: F,
    ) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(FunctionEnvMut<T>, &[Value]) -> Fut + 'static + Send + Sync,
        Fut: Future<Output = Result<Vec<Value>, RuntimeError>> + 'static + Send,
    {
        let function_type = ty.into();
        let func_ty = function_type.clone();
        let func_env = env.clone().into_sys();
        let raw_store = store.as_store_mut().as_raw() as *mut StoreInner;
        let wrapper = move |values_vec: *mut RawValue| -> HostCallOutcome {
            unsafe {
                let mut store = StoreMut::from_raw(raw_store);
                let mut args = Vec::with_capacity(func_ty.params().len());

                for (i, ty) in func_ty.params().iter().enumerate() {
                    args.push(Value::from_raw(
                        &mut store,
                        *ty,
                        values_vec.add(i).read_unaligned(),
                    ));
                }
                let store_mut = StoreMut::from_raw(raw_store);
                let env = env::FunctionEnvMut {
                    store_mut,
                    func_env: func_env.clone(),
                }
                .into();
                let sig = func_ty.clone();
                let future = func(env, &args);
                HostCallOutcome::Future {
                    func_ty: sig,
                    future: Box::pin(future)
                        as Pin<Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>> + Send>>,
                }
            }
        };
        let mut host_data = Box::new(VMDynamicFunctionContext {
            address: std::ptr::null(),
            ctx: DynamicFunction {
                func: wrapper,
                raw_store,
            },
        });
        host_data.address = host_data.ctx.func_body_ptr() as *const VMFunctionBody;

        let func_ptr = std::ptr::null() as VMFunctionCallback;
        let type_index = store
            .as_store_mut()
            .engine()
            .as_sys()
            .register_signature(&function_type);
        let vmctx = VMFunctionContext {
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
            handle: StoreHandle::new(store.as_store_mut().objects_mut().as_sys_mut(), vm_function),
        }
    }

    /// Creates a new host `Function` from a native function.
    pub(crate) fn new_typed<F, Args, Rets>(store: &mut impl AsStoreMut, func: F) -> Self
    where
        F: HostFunction<(), Args, Rets, WithoutEnv> + 'static + Send + Sync,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        let env = FunctionEnv::new(store, ());
        let func_ptr = func.function_callback_sys().into_sys();
        let host_data = Box::new(StaticFunction {
            raw_store: store.as_store_mut().as_raw() as *mut u8,
            env,
            func,
        });
        let function_type = FunctionType::new(Args::wasm_types(), Rets::wasm_types());

        let type_index = store
            .as_store_mut()
            .engine()
            .as_sys()
            .register_signature(&function_type);
        let vmctx = VMFunctionContext {
            host_env: host_data.as_ref() as *const _ as *mut c_void,
        };
        let call_trampoline =
            <F as HostFunction<(), Args, Rets, WithoutEnv>>::call_trampoline_address().into_sys();
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
            handle: StoreHandle::new(store.as_store_mut().objects_mut().as_sys_mut(), vm_function),
        }
    }

    pub(crate) fn new_typed_async<F, Fut, Args, Rets, RetsAsResult>(
        store: &mut impl AsStoreMut,
        func: F,
    ) -> Self
    where
        F: Fn(Args) -> Fut + 'static + Send + Sync,
        Fut: Future<Output = RetsAsResult> + 'static + Send,
        Args: WasmTypeList,
        Rets: WasmTypeList,
        RetsAsResult: IntoResult<Rets>,
    {
        let env = FunctionEnv::new(store, ());
        let func = Arc::new(func);
        Self::new_typed_with_env_async(store, &env, move |_env, args| {
            let func = func.clone();
            func(args)
        })
    }

    pub(crate) fn new_typed_with_env_async<T, F, Fut, Args, Rets, RetsAsResult>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        func: F,
    ) -> Self
    where
        T: Send + 'static,
        F: Fn(FunctionEnvMut<T>, Args) -> Fut + 'static + Send + Sync,
        Fut: Future<Output = RetsAsResult> + 'static + Send,
        Args: WasmTypeList,
        Rets: WasmTypeList,
        RetsAsResult: IntoResult<Rets>,
    {
        let signature = FunctionType::new(Args::wasm_types(), Rets::wasm_types());
        let args_sig = Arc::new(signature.clone());
        let results_sig = Arc::new(signature.clone());
        let func = Arc::new(func);
        Self::new_with_env_async(store, env, signature, move |mut env_mut, values| -> Pin<
            Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>> + Send>,
        > {
            let raw_store = RawStorePtr {
                ptr: env_mut.as_store_mut().as_raw(),
            };
            let args_sig = args_sig.clone();
            let results_sig = results_sig.clone();
            let func = func.clone();
            let args = match typed_args_from_values::<Args>(raw_store, args_sig.as_ref(), values) {
                Ok(args) => args,
                Err(err) => return Box::pin(async { Err(err) }),
            };
            let future = (*func)(env_mut, args);
            Box::pin(async move {
                let typed_result = future
                    .await
                    .into_result()
                    .map_err(|err| RuntimeError::user(Box::new(err)))?;
                typed_results_to_values::<Rets>(raw_store, results_sig.as_ref(), typed_result)
            })
        })
    }

    pub(crate) fn new_typed_with_env<T: Send + 'static, F, Args, Rets>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        func: F,
    ) -> Self
    where
        F: HostFunction<T, Args, Rets, WithEnv> + 'static + Send + Sync,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        let func_ptr = func.function_callback_sys().into_sys();
        let host_data = Box::new(StaticFunction {
            raw_store: store.as_store_mut().as_raw() as *mut u8,
            env: env.as_sys().clone().into(),
            func,
        });
        let function_type = FunctionType::new(Args::wasm_types(), Rets::wasm_types());

        let type_index = store
            .as_store_mut()
            .engine()
            .as_sys()
            .register_signature(&function_type);
        let vmctx = VMFunctionContext {
            host_env: host_data.as_ref() as *const _ as *mut c_void,
        };
        let call_trampoline =
            <F as HostFunction<T, Args, Rets, WithEnv>>::call_trampoline_address().into_sys();
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
            handle: StoreHandle::new(store.as_store_mut().objects_mut().as_sys_mut(), vm_function),
        }
    }

    pub(crate) fn ty(&self, store: &impl AsStoreRef) -> FunctionType {
        self.handle
            .get(store.as_store_ref().objects().as_sys())
            .signature
            .clone()
    }

    fn call_wasm(
        &self,
        store: &mut impl AsStoreMut,
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
        let signature = self.ty(store);
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
            if !arg.is_from_store(store) {
                return Err(RuntimeError::new("cross-`Store` values are not supported"));
            }
            *slot = arg.as_raw(store);
        }

        // Invoke the call
        self.call_wasm_raw(store, trampoline, values_vec, results)?;
        Ok(())
    }

    fn call_wasm_raw(
        &self,
        store: &mut impl AsStoreMut,
        trampoline: VMTrampoline,
        mut params: Vec<RawValue>,
        results: &mut [Value],
    ) -> Result<(), RuntimeError> {
        // Call the trampoline.
        let result = {
            let mut r;
            // TODO: This loop is needed for asyncify. It will be refactored with https://github.com/wasmerio/wasmer/issues/3451
            loop {
                let storeref = store.as_store_ref();
                let vm_function = self.handle.get(storeref.objects().as_sys());
                let config = storeref.engine().tunables().vmconfig();
                r = unsafe {
                    wasmer_call_trampoline(
                        store.as_store_ref().signal_handler(),
                        config,
                        vm_function.anyfunc.as_ptr().as_ref().vmctx,
                        trampoline,
                        vm_function.anyfunc.as_ptr().as_ref().func_ptr,
                        params.as_mut_ptr() as *mut u8,
                    )
                };
                let store_mut = store.as_store_mut();
                if let Some(callback) = store_mut.inner.on_called.take() {
                    match callback(store_mut) {
                        Ok(wasmer_types::OnCalledAction::InvokeAgain) => {
                            continue;
                        }
                        Ok(wasmer_types::OnCalledAction::Finish) => {
                            break;
                        }
                        Ok(wasmer_types::OnCalledAction::Trap(trap)) => {
                            return Err(RuntimeError::user(trap));
                        }
                        Err(trap) => return Err(RuntimeError::user(trap)),
                    }
                }
                break;
            }
            r
        };
        if let Err(error) = result {
            return Err(error.into());
        }

        // Load the return values out of `values_vec`.
        let signature = self.ty(store);
        for (index, &value_type) in signature.results().iter().enumerate() {
            unsafe {
                results[index] = Value::from_raw(store, value_type, params[index]);
            }
        }

        Ok(())
    }

    pub(crate) fn result_arity(&self, store: &impl AsStoreRef) -> usize {
        self.ty(store).results().len()
    }

    pub(crate) fn call(
        &self,
        store: &mut impl AsStoreMut,
        params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        let trampoline = unsafe {
            self.handle
                .get(store.as_store_ref().objects().as_sys())
                .anyfunc
                .as_ptr()
                .as_ref()
                .call_trampoline
        };
        let mut results = vec![Value::null(); self.result_arity(store)];
        self.call_wasm(store, trampoline, params, &mut results)?;
        Ok(results.into_boxed_slice())
    }

    pub(crate) fn call_async<'a>(
        &self,
        store: &'a mut (impl AsStoreMut + 'static),
        params: Vec<Value>,
    ) -> Pin<Box<dyn Future<Output = Result<Box<[Value]>, RuntimeError>> + 'a>> {
        let function = self.clone();
        Box::pin(call_function_async(function, store, params))
    }

    #[doc(hidden)]
    #[allow(missing_docs)]
    pub(crate) fn call_raw(
        &self,
        store: &mut impl AsStoreMut,
        params: Vec<RawValue>,
    ) -> Result<Box<[Value]>, RuntimeError> {
        let trampoline = unsafe {
            self.handle
                .get(store.as_store_ref().objects().as_sys())
                .anyfunc
                .as_ptr()
                .as_ref()
                .call_trampoline
        };
        let mut results = vec![Value::null(); self.result_arity(store)];
        self.call_wasm_raw(store, trampoline, params, &mut results)?;
        Ok(results.into_boxed_slice())
    }

    pub(crate) fn vm_funcref(&self, store: &impl AsStoreRef) -> VMFuncRef {
        let vm_function = self.handle.get(store.as_store_ref().objects().as_sys());
        if vm_function.kind == VMFunctionKind::Dynamic {
            panic!("dynamic functions cannot be used in tables or as funcrefs");
        }
        VMFuncRef(vm_function.anyfunc.as_ptr())
    }

    pub(crate) unsafe fn from_vm_funcref(store: &mut impl AsStoreMut, funcref: VMFuncRef) -> Self {
        let signature = {
            let anyfunc = unsafe { funcref.0.as_ref() };
            store
                .as_store_ref()
                .engine()
                .as_sys()
                .lookup_signature(anyfunc.type_index)
                .expect("Signature not found in store")
        };
        let vm_function = VMFunction {
            anyfunc: MaybeInstanceOwned::Instance(funcref.0),
            signature,
            // All functions in tables are already Static (as dynamic functions
            // are converted to use the trampolines with static signatures).
            kind: wasmer_vm::VMFunctionKind::Static,
            host_data: Box::new(()),
        };
        Self {
            handle: StoreHandle::new(store.objects_mut().as_sys_mut(), vm_function),
        }
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternFunction) -> Self {
        Self {
            handle: unsafe {
                StoreHandle::from_internal(
                    store.as_store_ref().objects().id(),
                    vm_extern.into_sys(),
                )
            },
        }
    }

    /// Checks whether this `Function` can be used with the given store.
    pub(crate) fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.handle.store_id() == store.as_store_ref().objects().id()
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Sys(wasmer_vm::VMExtern::Function(self.handle.internal_handle()))
    }
}

// We want to keep as much logic as possible on the host stack,
// since the WASM stack may be out of memory. In that scenario,
// throwing exceptions won't work since libunwind requires
// considerable stack space to do its magic, but everything else
// should work.
enum InvocationResult<T, E> {
    Success(T),
    Exception(crate::Exception),
    Trap(Box<E>),
    YieldOutsideAsyncContext,
}

fn to_invocation_result<T, E>(result: Result<T, E>) -> InvocationResult<T, E>
where
    E: Error + 'static,
{
    match result {
        Ok(value) => InvocationResult::Success(value),
        Err(trap) => {
            let dyn_err_ref = &trap as &dyn Error;
            if let Some(runtime_error) = dyn_err_ref.downcast_ref::<RuntimeError>()
                && let Some(exception) = runtime_error.to_exception()
            {
                return InvocationResult::Exception(exception);
            }
            InvocationResult::Trap(Box::new(trap))
        }
    }
}

fn write_dynamic_results(
    raw_store: *mut StoreInner,
    func_ty: &FunctionType,
    returns: Vec<Value>,
    values_vec: *mut RawValue,
) -> Result<(), RuntimeError> {
    let mut store = unsafe { StoreMut::from_raw(raw_store) };
    let return_types = returns.iter().map(|ret| ret.ty());
    if return_types.ne(func_ty.results().iter().copied()) {
        return Err(RuntimeError::new(format!(
            "Dynamic function returned wrong signature. Expected {:?} but got {:?}",
            func_ty.results(),
            returns.iter().map(|ret| ret.ty())
        )));
    }
    for (i, ret) in returns.iter().enumerate() {
        unsafe {
            values_vec.add(i).write_unaligned(ret.as_raw(&store));
        }
    }
    Ok(())
}

fn finalize_dynamic_call(
    raw_store: *mut StoreInner,
    func_ty: FunctionType,
    values_vec: *mut RawValue,
    result: Result<Vec<Value>, RuntimeError>,
) -> Result<(), RuntimeError> {
    match result {
        Ok(values) => write_dynamic_results(raw_store, &func_ty, values, values_vec),
        Err(err) => Err(err),
    }
}

#[derive(Clone, Copy)]
struct RawStorePtr {
    ptr: *mut StoreInner,
}

unsafe impl Send for RawStorePtr {}
unsafe impl Sync for RawStorePtr {}

fn typed_args_from_values<Args>(
    raw_store: RawStorePtr,
    func_ty: &FunctionType,
    values: &[Value],
) -> Result<Args, RuntimeError>
where
    Args: WasmTypeList,
{
    if values.len() != func_ty.params().len() {
        return Err(RuntimeError::new(
            "typed host function received wrong number of parameters",
        ));
    }
    let mut store = unsafe { StoreMut::from_raw(raw_store.ptr) };
    let mut raw_array = Args::empty_array();
    for ((slot, value), expected_ty) in raw_array
        .as_mut()
        .iter_mut()
        .zip(values.iter())
        .zip(func_ty.params().iter())
    {
        debug_assert_eq!(
            value.ty(),
            *expected_ty,
            "wasm should only call host functions with matching signatures"
        );
        *slot = value.as_raw(&store);
    }
    unsafe { Ok(Args::from_array(&mut store, raw_array)) }
}

fn typed_results_to_values<Rets>(
    raw_store: RawStorePtr,
    func_ty: &FunctionType,
    rets: Rets,
) -> Result<Vec<Value>, RuntimeError>
where
    Rets: WasmTypeList,
{
    let mut store = unsafe { StoreMut::from_raw(raw_store.ptr) };
    let mut raw_array = unsafe { rets.into_array(&mut store) };
    let mut values = Vec::with_capacity(func_ty.results().len());
    for (raw, ty) in raw_array.as_mut().iter().zip(func_ty.results().iter()) {
        unsafe {
            values.push(Value::from_raw(&mut store, *ty, *raw));
        }
    }
    Ok(values)
}

pub(crate) enum HostCallOutcome {
    Ready {
        func_ty: FunctionType,
        result: Result<Vec<Value>, RuntimeError>,
    },
    Future {
        func_ty: FunctionType,
        future: Pin<Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>> + Send>>,
    },
}

/// Host state for a dynamic function.
pub(crate) struct DynamicFunction<F> {
    func: F,
    raw_store: *mut StoreInner,
}

impl<F> DynamicFunction<F>
where
    F: Fn(*mut RawValue) -> HostCallOutcome + 'static,
{
    // This function wraps our func, to make it compatible with the
    // reverse trampoline signature
    unsafe extern "C-unwind" fn func_wrapper(
        this: &mut VMDynamicFunctionContext<Self>,
        values_vec: *mut RawValue,
    ) {
        let result = on_host_stack(|| {
            panic::catch_unwind(AssertUnwindSafe(|| match (this.ctx.func)(values_vec) {
                HostCallOutcome::Ready { func_ty, result } => to_invocation_result(
                    finalize_dynamic_call(this.ctx.raw_store, func_ty, values_vec, result),
                ),
                HostCallOutcome::Future { func_ty, future } => {
                    let awaited = block_on_host_future(future);
                    let result = match awaited {
                        Ok(value) => Ok(value),
                        Err(AsyncRuntimeError::RuntimeError(e)) => Err(e),
                        Err(AsyncRuntimeError::YieldOutsideAsyncContext) => {
                            return InvocationResult::YieldOutsideAsyncContext;
                        }
                    };
                    to_invocation_result(finalize_dynamic_call(
                        this.ctx.raw_store,
                        func_ty,
                        values_vec,
                        result,
                    ))
                }
            }))
        });

        // IMPORTANT: DO NOT ALLOCATE ON THE STACK,
        // AS WE ARE IN THE WASM STACK, NOT ON THE HOST ONE.
        // See: https://github.com/wasmerio/wasmer/pull/5700
        match result {
            Ok(InvocationResult::Success(())) => {}
            Ok(InvocationResult::Exception(exception)) => unsafe {
                let store = StoreMut::from_raw(this.ctx.raw_store);
                wasmer_vm::libcalls::throw(
                    store.as_store_ref().objects().as_sys(),
                    exception.vm_exceptionref().as_sys().to_u32_exnref(),
                )
            },
            Ok(InvocationResult::Trap(trap)) => unsafe { raise_user_trap(trap) },
            Ok(InvocationResult::YieldOutsideAsyncContext) => unsafe {
                raise_lib_trap(Trap::lib(TrapCode::YieldOutsideAsyncContext))
            },
            Err(panic) => unsafe { resume_panic(panic) },
        }
    }

    fn func_body_ptr(&self) -> VMFunctionCallback {
        Self::func_wrapper as VMFunctionCallback
    }

    fn call_trampoline_address(&self) -> VMTrampoline {
        Self::call_trampoline
    }

    unsafe extern "C" fn call_trampoline(
        vmctx: *mut VMContext,
        _body: VMFunctionCallback,
        args: *mut RawValue,
    ) {
        // The VMFunctionCallback is null here: it is only filled in later
        // by the engine linker.
        unsafe {
            let dynamic_function = &mut *(vmctx as *mut VMDynamicFunctionContext<Self>);
            Self::func_wrapper(dynamic_function, args);
        }
    }
}

/// Represents a low-level Wasm static host function. See
/// [`crate::Function::new_typed`] and
/// [`crate::Function::new_typed_with_env`] to learn more.
pub(crate) struct StaticFunction<F, T> {
    pub(crate) raw_store: *mut u8,
    pub(crate) env: FunctionEnv<T>,
    pub(crate) func: F,
}

impl crate::Function {
    /// Consume [`self`] into [`crate::backend::sys::function::Function`].
    pub fn into_sys(self) -> crate::backend::sys::function::Function {
        match self.0 {
            BackendFunction::Sys(s) => s,
            _ => panic!("Not a `sys` function!"),
        }
    }

    /// Convert a reference to [`self`] into a reference to [`crate::backend::sys::function::Function`].
    pub fn as_sys(&self) -> &crate::backend::sys::function::Function {
        match self.0 {
            BackendFunction::Sys(ref s) => s,
            _ => panic!("Not a `sys` function!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::sys::function::Function`].
    pub fn as_sys_mut(&mut self) -> &mut crate::backend::sys::function::Function {
        match self.0 {
            BackendFunction::Sys(ref mut s) => s,
            _ => panic!("Not a `sys` function!"),
        }
    }
}

macro_rules! impl_host_function {
    ([$c_struct_representation:ident] $c_struct_name:ident, $( $x:ident ),* ) => {
        paste::paste! {
        #[allow(non_snake_case)]
        pub(crate) fn [<gen_fn_callback_ $c_struct_name:lower _no_env>]
            <$( $x: FromToNativeWasmType, )* Rets: WasmTypeList, RetsAsResult: IntoResult<Rets>, Func: Fn($( $x , )*) -> RetsAsResult + 'static>
            (this: &Func) -> crate::backend::sys::vm::VMFunctionCallback {
            /// This is a function that wraps the real host
            /// function. Its address will be used inside the
            /// runtime.
            unsafe extern "C-unwind" fn func_wrapper<$( $x, )* Rets, RetsAsResult, Func>( env: &StaticFunction<Func, ()>, $( $x: <$x::Native as NativeWasmType>::Abi, )* ) -> Rets::CStruct
            where
                $( $x: FromToNativeWasmType, )*
                Rets: WasmTypeList,
                RetsAsResult: IntoResult<Rets>,
                Func: Fn($( $x , )*) -> RetsAsResult + 'static,
            {
                let mut store = unsafe { StoreMut::from_raw(env.raw_store as *mut _) };
                let result = on_host_stack(|| {
                    panic::catch_unwind(AssertUnwindSafe(|| {
                        $(
                            let $x = unsafe {
                                FromToNativeWasmType::from_native(NativeWasmTypeInto::from_abi(&mut store, $x))
                            };
                        )*
                        to_invocation_result((env.func)($($x),* ).into_result())
                    }))
                });

                // IMPORTANT: DO NOT ALLOCATE ON THE STACK,
                // AS WE ARE IN THE WASM STACK, NOT ON THE HOST ONE.
                // See: https://github.com/wasmerio/wasmer/pull/5700
                match result {
                    Ok(InvocationResult::Success(result)) => {
                        unsafe {
                            return result.into_c_struct(&mut store);
                        }
                    },
                    Ok(InvocationResult::Exception(exception)) => unsafe {
                        wasmer_vm::libcalls::throw(
                            store.as_store_ref().objects().as_sys(),
                            exception.vm_exceptionref().as_sys().to_u32_exnref()
                        )
                    }
                    Ok(InvocationResult::Trap(trap)) => unsafe { raise_user_trap(trap) },
                    Ok(InvocationResult::YieldOutsideAsyncContext) => unsafe {
                        raise_lib_trap(Trap::lib(TrapCode::YieldOutsideAsyncContext))
                    },
                    Err(panic) => unsafe { resume_panic(panic) },
                }
            }

            func_wrapper::< $( $x, )* Rets, RetsAsResult, Func > as _

        }

        #[allow(non_snake_case)]
        pub(crate) fn [<gen_call_trampoline_address_ $c_struct_name:lower _no_env>]
            <$( $x: FromToNativeWasmType, )* Rets: WasmTypeList>
            () -> crate::backend::sys::vm::VMTrampoline {

            unsafe extern "C" fn call_trampoline<$( $x: FromToNativeWasmType, )* Rets: WasmTypeList>
            (
                vmctx: *mut crate::backend::sys::vm::VMContext,
                body: crate::backend::sys::vm::VMFunctionCallback,
                args: *mut RawValue,
            ) {
                let mut _n = 0;

                unsafe {
                    let body: unsafe extern "C" fn(vmctx: *mut crate::backend::sys::vm::VMContext, $( $x: <$x::Native as NativeWasmType>::Abi, )*) -> Rets::CStruct = std::mem::transmute(body);
                    $(
                        let $x = *args.add(_n).cast();
                        _n += 1;
                    )*
                    let results = body(vmctx, $( $x ),*);
                    Rets::write_c_struct_to_ptr(results, args);
                }
            }

            call_trampoline::<$( $x, )* Rets> as _

        }

        #[allow(non_snake_case)]
        pub(crate) fn [<gen_fn_callback_ $c_struct_name:lower>]
            <$( $x: FromToNativeWasmType, )* Rets: WasmTypeList, RetsAsResult: IntoResult<Rets>, T: Send + 'static,  Func: Fn(FunctionEnvMut<T>, $( $x , )*) -> RetsAsResult + 'static>
            (this: &Func) -> crate::backend::sys::vm::VMFunctionCallback {
            /// This is a function that wraps the real host
            /// function. Its address will be used inside the
            /// runtime.
            unsafe extern "C-unwind" fn func_wrapper<T: Send + 'static, $( $x, )* Rets, RetsAsResult, Func>( env: &StaticFunction<Func, T>, $( $x: <$x::Native as NativeWasmType>::Abi, )* ) -> Rets::CStruct
                where
                $( $x: FromToNativeWasmType, )*
                Rets: WasmTypeList,
                RetsAsResult: IntoResult<Rets>,
                Func: Fn(FunctionEnvMut<T>, $( $x , )*) -> RetsAsResult + 'static,
            {

                let mut store = unsafe { StoreMut::from_raw(env.raw_store as *mut _) };
                let result = wasmer_vm::on_host_stack(|| {
                    panic::catch_unwind(AssertUnwindSafe(|| {
                        $(
                            let $x = unsafe {
                                FromToNativeWasmType::from_native(NativeWasmTypeInto::from_abi(&mut store, $x))
                            };
                        )*
                        let store_mut = unsafe { StoreMut::from_raw(env.raw_store as *mut _) };
                        let f_env = crate::backend::sys::function::env::FunctionEnvMut {
                            store_mut,
                            func_env: env.env.as_sys().clone(),
                        }.into();
                        to_invocation_result((env.func)(f_env, $($x),* ).into_result())
                    }))
                });

                // IMPORTANT: DO NOT ALLOCATE ON THE STACK,
                // AS WE ARE IN THE WASM STACK, NOT ON THE HOST ONE.
                // See: https://github.com/wasmerio/wasmer/pull/5700
                match result {
                    Ok(InvocationResult::Success(result)) => {
                        unsafe {
                            return result.into_c_struct(&mut store);
                        }
                    },
                    Ok(InvocationResult::Exception(exception)) => unsafe {
                        wasmer_vm::libcalls::throw(
                            store.as_store_ref().objects().as_sys(),
                            exception.vm_exceptionref().as_sys().to_u32_exnref()
                        )
                    }
                    Ok(InvocationResult::Trap(trap)) => unsafe { raise_user_trap(trap) },
                    Ok(InvocationResult::YieldOutsideAsyncContext) => unsafe {
                        raise_lib_trap(Trap::lib(TrapCode::YieldOutsideAsyncContext))
                    },
                    Err(panic) => unsafe { resume_panic(panic) },
                }
            }
            func_wrapper::< T, $( $x, )* Rets, RetsAsResult, Func > as _
        }

        #[allow(non_snake_case)]
        pub(crate) fn [<gen_call_trampoline_address_ $c_struct_name:lower>]
            <$( $x: FromToNativeWasmType, )* Rets: WasmTypeList>
            () -> crate::backend::sys::vm::VMTrampoline {

            unsafe extern "C" fn call_trampoline<$( $x: FromToNativeWasmType, )* Rets: WasmTypeList>(
                  vmctx: *mut crate::backend::sys::vm::VMContext,
                  body: crate::backend::sys::vm::VMFunctionCallback,
                  args: *mut RawValue,
            ) {
                unsafe {
                    let body: unsafe extern "C" fn(vmctx: *mut crate::backend::sys::vm::VMContext, $( $x: <$x::Native as NativeWasmType>::Abi, )*) -> Rets::CStruct = std::mem::transmute(body);
                    let mut _n = 0;
                    $(
                    let $x = *args.add(_n).cast();
                    _n += 1;
                    )*

                    let results = body(vmctx, $( $x ),*);

                    Rets::write_c_struct_to_ptr(results, args);
                }
            }

            call_trampoline::<$( $x, )* Rets> as _
        }
    }};
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
