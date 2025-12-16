//! Data types, functions and traits for `sys` runtime's `Function` implementation.

pub(crate) mod env;
pub(crate) mod typed;

use crate::{
    BackendFunction, Continuation, Exception, FunctionEnv, FunctionEnvMut, FunctionType, HostFunction, RuntimeError, StoreInner, Tag, Value, WithEnv, WithoutEnv, backend::sys::{engine::NativeEngineExt, vm::VMFunctionCallback}, entities::store::{AsStoreMut, AsStoreRef, StoreMut}, utils::{FromToNativeWasmType, IntoResult, NativeWasmTypeInto, WasmTypeList}, vm::{VMExtern, VMExternFunction}
};
use std::panic::{self, AssertUnwindSafe};
use std::{cell::UnsafeCell, cmp::max, error::Error, ffi::c_void};
use wasmer_types::{NativeWasmType, RawValue, Type};
use wasmer_vm::{
    MaybeInstanceOwned, StoreHandle, Trap, VMCallerCheckedAnyfunc, VMContext, VMContinuationRef,
    VMDynamicFunctionContext, VMFuncRef, VMFunction, VMFunctionBody, VMFunctionContext,
    VMFunctionKind, VMTrampoline, host_call_trampoline, host_call_trampoline_resume, on_host_stack,
    on_separate_host_stack, raise_lib_trap, raise_user_trap, resume_panic, wasmer_call_trampoline,
    wasmer_call_trampoline_resume,
};

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
        let raw_store = store.as_store_mut().as_raw() as *mut u8;
        let wrapper = move |values_vec: *mut RawValue| -> Result<(), RuntimeError> {
            unsafe {
                let mut store = StoreMut::from_raw(raw_store as *mut StoreInner);
                let mut args = Vec::with_capacity(func_ty.params().len());

                for (i, ty) in func_ty.params().iter().enumerate() {
                    args.push(Value::from_raw(
                        &mut store,
                        *ty,
                        values_vec.add(i).read_unaligned(),
                    ));
                }
                let store_mut = StoreMut::from_raw(raw_store as *mut StoreInner);
                let env = env::FunctionEnvMut {
                    store_mut,
                    func_env: func_env.clone(),
                }
                .into();
                let returns = func(env, &args)?;

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
                    values_vec.add(i).write_unaligned(ret.as_raw(&store));
                }
            }
            Ok(())
        };
        let mut host_data = Box::new(VMDynamicFunctionContext {
            address: std::ptr::null(),
            ctx: DynamicFunction {
                func: wrapper,
                raw_store,
            },
        });
        host_data.address = host_data.ctx.func_body_ptr();

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

    fn call_wasm_resume(
        &self,
        store: &mut impl AsStoreMut,
        // TODO: Figure out if this should be a crate::vm::VMContinuationRef or a wasmer_vm::VMContinuationRef
        continuation: wasmer_vm::VMContinuationRef,
        results: &mut [Value],
    ) -> Result<(), RuntimeError> {
        // let format_types_for_error_message = |items: &[Value]| {
        //     items
        //         .iter()
        //         .map(|param| param.ty().to_string())
        //         .collect::<Vec<String>>()
        //         .join(", ")
        // };
        // // TODO: Avoid cloning the signature here, it's expensive.
        // let signature = self.ty(store);
        // if signature.params().len() != params.len() {
        //     return Err(RuntimeError::new(format!(
        //         "Parameters of type [{}] did not match signature {}",
        //         format_types_for_error_message(params),
        //         &signature
        //     )));
        // }
        // if signature.results().len() != results.len() {
        //     return Err(RuntimeError::new(format!(
        //         "Results of type [{}] did not match signature {}",
        //         format_types_for_error_message(results),
        //         &signature,
        //     )));
        // }

        // let mut values_vec = vec![RawValue { i32: 0 }; max(params.len(), results.len())];

        // // Store the argument values into `values_vec`.
        // let param_tys = signature.params().iter();
        // for ((arg, slot), ty) in params.iter().zip(&mut values_vec).zip(param_tys) {
        //     if arg.ty() != *ty {
        //         let param_types = format_types_for_error_message(params);
        //         return Err(RuntimeError::new(format!(
        //             "Parameters of type [{}] did not match signature {}",
        //             param_types, &signature,
        //         )));
        //     }
        //     if !arg.is_from_store(store) {
        //         return Err(RuntimeError::new("cross-`Store` values are not supported"));
        //     }
        //     *slot = arg.as_raw(store);
        // }

        // Invoke the call

        // TODO: This currently does absolutely no checks that the continuation matches the function or the store
        self.call_wasm_raw_resume(store, continuation, results)?;
        Ok(())
    }

    fn call_wasm_raw(
        &self,
        store: &mut impl AsStoreMut,
        trampoline: VMTrampoline,
        // TODO: Params probably die at the end of this function. This could be a problem
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
            eprintln!("Function call resulted in error: {:?}", error);
            if let Trap::Continuation { continuation } = &error {
                eprintln!("The error is a continuation");
                // If the error is a continuation
                // 1. Transfer ownership of the params to the continuation
                // 2. Store the result types in the continuation for later use
                let signature = self.ty(store);
                let result_types = signature.results().to_vec();

                // Store the results pointer and types in the continuation
                let mut continuation_mut = continuation
                    .0
                    .get_mut(store.as_store_mut().inner.objects.as_sys_mut());
                assert!(continuation_mut.params_vec.is_none());
                assert!(continuation_mut.result_types.is_none());
                continuation_mut.params_vec = Some(params);
                continuation_mut.result_types = Some(result_types);
            }

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

    // warn on unused vars in this functions
    fn call_wasm_raw_resume(
        &self,
        store: &mut impl AsStoreMut,
        continuation: wasmer_vm::VMContinuationRef,
        results: &mut [Value],
    ) -> Result<(), RuntimeError> {
        // Call the trampoline.
        let result = {
            let mut r;
            // TODO: This loop is needed for asyncify. It will be refactored with https://github.com/wasmerio/wasmer/issues/3451
            loop {
                // let storeref = store.as_store_ref();
                // let vm_function = self.handle.get(storeref.objects().as_sys());
                // let config = storeref.engine().tunables().vmconfig();
                // let storeref = store.as_store_ref();
                // let vm_function = self.handle.get(storeref.objects().as_sys());
                // let store_objects = store.objects_mut().as_sys_mut();
                r = unsafe {
                    wasmer_call_trampoline_resume(
                        store.as_store_ref().signal_handler(),
                        // TODO: Passs store directly here
                        store.objects_mut().as_sys_mut(),
                        continuation.clone(),
                    )
                };

                // TODO: Continuations should be safe as long as this is never triggered
                //       I add added a todo so this is a guaranteed error
                let store_mut = store.as_store_mut();
                if let Some(callback) = store_mut.inner.on_called.take() {
                    todo!("Evaluate if this works with continuations");
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
            // Continuations should return here if they suspend again
            return Err(error.into());
        }

        let mut continuation_mut = continuation
            .0
            .get(store.as_store_mut().inner.objects.as_sys());
        // Vector with results
        // cloned so we can use the store reference
        let result_values = continuation_mut.params_vec.as_ref().unwrap().clone();
        let result_types = continuation_mut.result_types.as_ref().unwrap().clone();

        for (index, &value_type) in result_types.iter().enumerate() {
            unsafe {
                results[index] = Value::from_raw(store, value_type, result_values[index]);
            }
        }
        eprintln!("This would have paniced without return value support");
        // // Load the return values out of `values_vec`.
        // let signature = self.ty(store);

        // for (index, &value_type) in signature.results().iter().enumerate() {
        //     unsafe {
        //         results[index] = Value::from_raw(store, value_type, RawValue { i32: 0 });
        //     }
        // }

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

    pub(crate) fn call_resume(
        &self,
        store: &mut impl AsStoreMut,
        // TODO: Figure out if this should be a crate::vm::VMContinuationRef or a wasmer_vm::VMContinuationRef
        continuation: crate::vm::VMContinuationRef,
    ) -> Result<Box<[Value]>, RuntimeError> {
        // let trampoline = unsafe {
        //     self.handle
        //         .get(store.as_store_ref().objects().as_sys())
        //         .anyfunc
        //         .as_ptr()
        //         .as_ref()
        //         .call_trampoline
        // };
        let mut results = vec![Value::null(); self.result_arity(store)];
        self.call_wasm_resume(store, continuation.into_sys(), &mut results)?;
        Ok(results.into_boxed_slice())
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

    #[doc(hidden)]
    #[allow(missing_docs)]
    pub(crate) fn call_raw_resume(
        &self,
        store: &mut impl AsStoreMut,
        continuation: wasmer_vm::VMContinuationRef,
    ) -> Result<Box<[Value]>, RuntimeError> {
        let mut results = vec![Value::null(); self.result_arity(store)];
        self.call_wasm_raw_resume(store, continuation, &mut results)?;
        todo!("Continuations are not allowed to return for now");
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
    Continuation(crate::Continuation),
    Trap(Box<E>),
}

// fn to_invocation_result_beta<T, E>(result: Result<T, Trap>) -> InvocationResult<T, Trap>
// where
//     E: Error + 'static,
// {
//     match result {
//         Ok(value) => InvocationResult::Success(value),
//         Err(trap) => {
//             let dyn_err_ref = &trap as &dyn Error;
//             if let Some(runtime_error) = dyn_err_ref.downcast_ref::<RuntimeError>() {
//                 if let Some(exception) = runtime_error.to_exception() {
//                     return InvocationResult::Exception(exception);
//                 }
//                 // TODO: let to_continuation return something meaningful
//                 if let Some(continuation) = runtime_error.to_continuation() {
//                     return InvocationResult::Continuation(
//                         continuation,
//                     );
//                 }
//             }
//             InvocationResult::Trap(Box::new(trap))
//         }
//     }
// }

fn trap_to_invocation_result<T, E>(result: Result<T, Trap>) -> InvocationResult<T, RuntimeError>
where
    E: Error + 'static,
{
    match result {
        Ok(value) => InvocationResult::Success(value),
        Err(trap) => {
                if let Some(exception) = trap.to_exception_ref() {
                    let e = Exception::from_vm_exceptionref(crate::vm::VMExceptionRef::Sys(exception));
                    return InvocationResult::Exception(e);
                }
                // TODO: let to_continuation return something meaningful
                if let Some(continuation) = trap.to_continuation_ref() {
                    let c = Continuation::from_vm_continuationref(crate::vm::VMContinuationRef::Sys(continuation));
                    return InvocationResult::Continuation(c);
                }
            InvocationResult::Trap(todo!())
        }
    }
}

fn to_invocation_result<T, E>(result: Result<T, E>) -> InvocationResult<T, E>
where
    E: Error + 'static,
{
    match result {
        Ok(value) => InvocationResult::Success(value),
        Err(trap) => {
            let dyn_err_ref = &trap as &dyn Error;
            if let Some(runtime_error) = dyn_err_ref.downcast_ref::<RuntimeError>() {
                if let Some(exception) = runtime_error.to_exception() {
                    return InvocationResult::Exception(exception);
                }
                // TODO: let to_continuation return something meaningful
                if let Some(continuation) = runtime_error.to_continuation() {
                    return InvocationResult::Continuation(continuation);
                }
            }
            InvocationResult::Trap(Box::new(trap))
        }
    }
}

/// Host state for a dynamic function.
pub(crate) struct DynamicFunction<F> {
    func: F,
    raw_store: *mut u8,
}

impl<F> DynamicFunction<F>
where
    F: Fn(*mut RawValue) -> Result<(), RuntimeError> + 'static,
{
    // This function wraps our func, to make it compatible with the
    // reverse trampoline signature
    unsafe extern "C-unwind" fn func_wrapper(
        this: &mut VMDynamicFunctionContext<Self>,
        values_vec: *mut RawValue,
    ) {
        let store = unsafe {
            StoreMut::from_raw(this.ctx.raw_store as *mut _)
                .as_store_mut()
                .objects_mut()
                .as_sys_mut()
        };
        let result: Result<Result<InvocationResult<(), RuntimeError>, _>, Trap> = unsafe {
            host_call_trampoline(store, || {
                let mut result = panic::catch_unwind(AssertUnwindSafe(|| {
                    to_invocation_result((this.ctx.func)(values_vec))
                }));
                let result2 = loop {
                    match result {
                        Ok(InvocationResult::Success(())) => {
                            break result;
                        }
                        Ok(InvocationResult::Exception(_)) => unsafe {
                            break result;
                        },
                        Ok(InvocationResult::Trap(_)) => unsafe {
                            break result;
                        },
                        Ok(InvocationResult::Continuation(continuation)) => unsafe {
                            let store = StoreMut::from_raw(this.ctx.raw_store as *mut _);
                            let other_payload = continuation.payload(&mut store.as_store_mut());
                            let tag = Tag::new(&mut store.as_store_mut(), [Type::I64]);
                            let new_continuation =
                                Continuation::new(&mut store.as_store_mut(), &tag, &other_payload);

                            wasmer_vm::libcalls::suspend(
                                store.as_store_ref().objects().as_sys(),
                                new_continuation
                                    .vm_continuation_ref()
                                    .as_sys()
                                    .to_u32_contref(),
                            );

                            let store_objects = store.as_store_mut().objects_mut().as_sys_mut();

                            result = panic::catch_unwind(AssertUnwindSafe(|| {
                                trap_to_invocation_result(host_call_trampoline_resume(
                                    None,
                                    store_objects,
                                    new_continuation.vm_continuation_ref().into_sys(),
                                ))
                            }));
                            ()
                        },
                        Err(panic) => unsafe { resume_panic(panic); unreachable!() },
                    }
                };
                result
            })
        };

        // IMPORTANT: DO NOT ALLOCATE ON THE STACK,
        // AS WE ARE IN THE WASM STACK, NOT ON THE HOST ONE.
        // See: https://github.com/wasmerio/wasmer/pull/5700
        match result {
            Ok(InvocationResult::Success(())) => {}
            Ok(InvocationResult::Exception(exception)) => unsafe {
                let store = StoreMut::from_raw(this.ctx.raw_store as *mut _);
                wasmer_vm::libcalls::throw(
                    store.as_store_ref().objects().as_sys(),
                    exception.vm_exceptionref().as_sys().to_u32_exnref(),
                )
            },
            Ok(InvocationResult::Trap(trap)) => unsafe { raise_user_trap(trap) },
            Ok(InvocationResult::Continuation(continuation)) => unsafe {
                let store = StoreMut::from_raw(this.ctx.raw_store as *mut _);
                wasmer_vm::libcalls::suspend(
                    store.as_store_ref().objects().as_sys(),
                    continuation.vm_continuation_ref().as_sys().to_u32_contref(),
                );
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
                    },
                    Ok(InvocationResult::Continuation(continuation)) => unsafe {
                        wasmer_vm::libcalls::suspend(
                            store.as_store_ref().objects().as_sys(),
                            continuation.vm_continuation_ref().as_sys().to_u32_contref(),
                        );
                        // TODO: When switching functions return a RuntimeError, so no success value is present
                        // Decide if we can do something better then returning a zeroed C-struct
                        return std::mem::zeroed::<Rets::CStruct>();

                    },
                    Ok(InvocationResult::Trap(trap)) => unsafe { raise_user_trap(trap) },
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
                    },
                    Ok(InvocationResult::Continuation(continuation)) => unsafe {
                        wasmer_vm::libcalls::suspend(
                            store.as_store_ref().objects().as_sys(),
                            continuation.vm_continuation_ref().as_sys().to_u32_contref(),
                        );
                        // TODO: When switching functions return a RuntimeError, so no success value is present
                        // Decide if we can do something better then returning a zeroed C-struct
                        return std::mem::zeroed::<Rets::CStruct>();
                    },
                    Ok(InvocationResult::Trap(trap)) => unsafe { raise_user_trap(trap) },
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
