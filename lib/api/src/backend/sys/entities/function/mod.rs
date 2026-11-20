//! Data types, functions and traits for `sys` runtime's `Function` implementation

pub(crate) mod env;
pub(crate) mod typed;

#[cfg(feature = "experimental-async")]
use crate::{
    AsStoreAsync, AsyncFunctionEnvMut, BackendAsyncFunctionEnvMut, StoreAsync, StoreInstallGuard,
    entities::function::async_host::{AsyncFunctionEnv, AsyncHostFunction},
    sys::{
        async_runtime::{
            AsyncRuntimeError, HostFuture, block_on_host_future, call_function_async,
            run_immediate,
        },
        function::env::AsyncFunctionEnvMutStore,
    },
};
use crate::{
    BackendFunction, CallWrapper, FunctionEnv, FunctionEnvMut, FunctionType, HostFunction,
    RuntimeError, SlotRef, StoreContext, StoreInner, StorePtrWrapper, Value, WithEnv, WithoutEnv,
    backend::sys::{engine::NativeEngineExt, vm::VMFunctionCallback},
    entities::function::call_wrapper::call_wrapped,
    entities::store::{AsStoreMut, AsStoreRef, StoreMut},
    utils::{FromToNativeWasmType, IntoResult, NativeWasmTypeInto, WasmTypeList},
    vm::{VMExtern, VMExternFunction},
};
use smallvec::{SmallVec, smallvec};
use std::panic::{self, AssertUnwindSafe};
use std::{
    any::Any,
    cell::UnsafeCell,
    cmp::max,
    error::Error,
    ffi::c_void,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicPtr, Ordering},
    },
};
use wasmer_types::{NativeWasmType, RawValue, StoreId, Type};
#[cfg(feature = "experimental-host-interrupt")]
use wasmer_vm::interrupt_registry;
use wasmer_vm::{
    MaybeInstanceOwned, StoreHandle, Trap, TrapCode, VMCallerCheckedAnyfunc, VMContext,
    VMDynamicFunctionContext, VMExceptionRef, VMFuncRef, VMFunction, VMFunctionBody,
    VMFunctionContext, VMFunctionKind, VMTrampoline, on_host_stack, raise_lib_trap,
    raise_user_trap, resume_panic, wasmer_call_trampoline,
};

// Buffers for a call's values, inline up to a size that covers most
// signatures, so that a call does not allocate for them.
type RawValues = SmallVec<[RawValue; 8]>;
type Values = SmallVec<[Value; 8]>;
type Types = SmallVec<[Type; 8]>;

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
        let store_id = store.objects_mut().id();
        let slot = SlotRef::of(store.objects_mut());
        let wrapper = move |values_vec: *mut RawValue| -> HostCallOutcome {
            unsafe {
                let mut store_wrapper = unsafe { StoreContext::get_current_in(slot, store_id) };
                let mut store_mut = store_wrapper.as_mut();
                let mut args = Values::new();
                for (i, ty) in func_ty.params().iter().enumerate() {
                    args.push(Value::from_raw(
                        &mut store_mut,
                        *ty,
                        values_vec.add(i).read_unaligned(),
                    ));
                }
                let env = env::FunctionEnvMut {
                    store_mut,
                    func_env: func_env.clone(),
                }
                .into();
                let result = func(env, &args);
                HostCallOutcome::Ready(result, store_wrapper)
            }
        };
        let mut host_data = Box::new(VMDynamicFunctionContext {
            address: std::ptr::null(),
            ctx: DynamicFunction {
                func: wrapper,
                store_id,
                slot: HostSlot::new(slot),
                func_ty: function_type.clone(),
                in_place: true,
            },
        });
        host_data.address = host_data.ctx.func_body_ptr();

        // We don't yet have the address with the Wasm ABI signature.
        // The engine linker will replace the address with one pointing to a
        // generated dynamic trampoline.
        let func_ptr = std::ptr::null() as VMFunctionCallback;
        let type_signature_hash = store
            .as_store_ref()
            .engine()
            .as_sys()
            .register_signature(&function_type);
        let vmctx = VMFunctionContext {
            host_env: host_data.as_ref() as *const _ as *mut c_void,
        };
        let call_trampoline = host_data.ctx.call_trampoline_address();
        let anyfunc = VMCallerCheckedAnyfunc {
            include_m0_param: false,
            func_ptr,
            type_signature_hash,
            vmctx,
            call_trampoline,
        };

        let in_place = host_data.ctx.in_place;
        let vm_function = VMFunction {
            anyfunc: MaybeInstanceOwned::Host(Box::new(UnsafeCell::new(anyfunc))),
            kind: VMFunctionKind::Dynamic,
            signature: function_type,
            host_data: HostFunctionData::boxed(host_data, |data| &data.ctx.slot, in_place),
        };
        Self {
            handle: StoreHandle::new(store.objects_mut().as_sys_mut(), vm_function),
        }
    }

    #[cfg(feature = "experimental-async")]
    pub(crate) fn new_async<FT, F, Fut>(store: &mut impl AsStoreMut, ty: FT, func: F) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(&[Value]) -> Fut + 'static,
        Fut: Future<Output = Result<Vec<Value>, RuntimeError>> + 'static,
    {
        let env = FunctionEnv::new(store, ());
        let wrapped = move |_env: AsyncFunctionEnvMut<()>, values: &[Value]| func(values);
        Self::new_with_env_async(store, &env, ty, wrapped)
    }

    #[cfg(feature = "experimental-async")]
    pub(crate) fn new_with_env_async<FT, F, Fut, T: 'static>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        ty: FT,
        func: F,
    ) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(AsyncFunctionEnvMut<T>, &[Value]) -> Fut + 'static,
        Fut: Future<Output = Result<Vec<Value>, RuntimeError>> + 'static,
    {
        Self::new_with_env_async_boxed(store, env, ty, move |env, values| -> HostFuture {
            Box::pin(func(env, values))
        })
    }

    /// [`Self::new_with_env_async`] for a function that boxes its future
    /// itself, as the typed wrappers do.
    #[cfg(feature = "experimental-async")]
    fn new_with_env_async_boxed<FT, F, T: 'static>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        ty: FT,
        func: F,
    ) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(AsyncFunctionEnvMut<T>, &[Value]) -> HostFuture + 'static,
    {
        let function_type = ty.into();
        let func_ty = function_type.clone();
        let func_env = env.clone().into_sys();
        let store_id = store.objects_mut().id();
        let slot = SlotRef::of(store.objects_mut());
        let wrapper = move |values_vec: *mut RawValue| -> HostCallOutcome {
            unsafe {
                // The environment of an async host function can be cloned
                // and kept anywhere, so its borrow of a plain store is counted
                // in an entry of the context stack, which checks that the
                // borrow was given back when it is removed: once the future
                // is done with.
                let plain_store_entry = slot
                    .get()
                    .map(|store_ptr| StoreContext::install(store_ptr.as_ptr()));
                let mut context = StoreContext::try_get_current_async(store_id);
                let mut store_mut = match &mut context {
                    crate::GetStoreAsyncGuardResult::Ok(wrapper) => StoreMut {
                        inner: wrapper.store_ptr.as_mut().unwrap(),
                    },
                    crate::GetStoreAsyncGuardResult::NotAsync(ptr) => ptr.as_mut(),
                    crate::GetStoreAsyncGuardResult::NotInstalled => {
                        panic!("No store context installed on this thread")
                    }
                };
                let id = store_mut.as_store_ref().objects().id();
                let mut args = Values::new();
                for (i, ty) in func_ty.params().iter().enumerate() {
                    args.push(Value::from_raw(
                        &mut store_mut,
                        *ty,
                        values_vec.add(i).read_unaligned(),
                    ));
                }
                let store_async = match context {
                    crate::GetStoreAsyncGuardResult::Ok(wrapper) => {
                        AsyncFunctionEnvMutStore::Async(StoreAsync {
                            id,
                            inner: crate::LocalRwLockWriteGuard::lock_handle(
                                wrapper.guard.as_mut().unwrap(),
                            ),
                        })
                    }
                    crate::GetStoreAsyncGuardResult::NotAsync(ptr) => {
                        AsyncFunctionEnvMutStore::Sync(ptr)
                    }
                    crate::GetStoreAsyncGuardResult::NotInstalled => unreachable!(),
                };
                let env = crate::AsyncFunctionEnvMut(crate::BackendAsyncFunctionEnvMut::Sys(
                    env::AsyncFunctionEnvMut {
                        store: store_async,
                        func_env: func_env.clone(),
                    },
                ));
                HostCallOutcome::Future(func(env, &args), plain_store_entry)
            }
        };
        let mut host_data = Box::new(VMDynamicFunctionContext {
            address: std::ptr::null(),
            ctx: DynamicFunction {
                func: wrapper,
                store_id,
                slot: HostSlot::new(slot),
                func_ty: function_type.clone(),
                // Its future may suspend the coroutine the call came from.
                in_place: false,
            },
        });
        host_data.address = host_data.ctx.func_body_ptr();

        let func_ptr = std::ptr::null() as VMFunctionCallback;
        let type_signature_hash = store
            .as_store_ref()
            .engine()
            .as_sys()
            .register_signature(&function_type);
        let vmctx = VMFunctionContext {
            host_env: host_data.as_ref() as *const _ as *mut c_void,
        };
        let call_trampoline = host_data.ctx.call_trampoline_address();
        let anyfunc = VMCallerCheckedAnyfunc {
            include_m0_param: false,
            func_ptr,
            type_signature_hash,
            vmctx,
            call_trampoline,
        };

        let in_place = host_data.ctx.in_place;
        let vm_function = VMFunction {
            anyfunc: MaybeInstanceOwned::Host(Box::new(UnsafeCell::new(anyfunc))),
            kind: VMFunctionKind::Dynamic,
            signature: function_type,
            host_data: HostFunctionData::boxed(host_data, |data| &data.ctx.slot, in_place),
        };
        Self {
            handle: StoreHandle::new(store.objects_mut().as_sys_mut(), vm_function),
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
        let func_ptr = func.function_callback_sys().unwrap_sys();
        let host_data = Box::new(StaticFunction {
            store_id: store.objects_mut().id(),
            slot: HostSlot::new(SlotRef::of(store.objects_mut())),
            env,
            func,
        });
        let function_type = FunctionType::new(Args::wasm_types(), Rets::wasm_types());

        let type_signature_hash = store
            .as_store_ref()
            .engine()
            .as_sys()
            .register_signature(&function_type);
        let vmctx = VMFunctionContext {
            host_env: host_data.as_ref() as *const _ as *mut c_void,
        };
        let call_trampoline =
            <F as HostFunction<(), Args, Rets, WithoutEnv>>::call_trampoline_address().unwrap_sys();
        let anyfunc = VMCallerCheckedAnyfunc {
            include_m0_param: false,
            func_ptr,
            type_signature_hash,
            vmctx,
            call_trampoline,
        };

        let vm_function = VMFunction {
            anyfunc: MaybeInstanceOwned::Host(Box::new(UnsafeCell::new(anyfunc))),
            kind: VMFunctionKind::Static,
            signature: function_type,
            host_data: HostFunctionData::boxed(host_data, |data| &data.slot, true),
        };
        Self {
            handle: StoreHandle::new(store.objects_mut().as_sys_mut(), vm_function),
        }
    }

    #[cfg(feature = "experimental-async")]
    pub(crate) fn new_typed_async<F, Args, Rets>(store: &mut impl AsStoreMut, func: F) -> Self
    where
        Args: WasmTypeList + 'static,
        Rets: WasmTypeList + 'static,
        F: AsyncHostFunction<(), Args, Rets, WithoutEnv> + 'static,
    {
        let env = FunctionEnv::new(store, ());
        let signature = FunctionType::new(Args::wasm_types(), Rets::wasm_types());
        let args_sig = Arc::new(signature.clone());
        let results_sig = Arc::new(signature.clone());
        let func = Arc::new(func);
        Self::new_with_env_async_boxed(
            store,
            &env,
            signature,
            move |mut env_mut,
                  values|
                  -> Pin<Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>>>> {
                let sys_env = match env_mut.0 {
                    BackendAsyncFunctionEnvMut::Sys(ref mut sys_env) => sys_env,
                    _ => panic!("Not a sys backend"),
                };
                let mut store_mut_wrapper =
                    unsafe { StoreContext::get_current(sys_env.store_id()) };
                let mut store_mut = store_mut_wrapper.as_mut();
                let args_sig = args_sig.clone();
                let results_sig = results_sig.clone();
                let func = func.clone();
                let args =
                    match typed_args_from_values::<Args>(&mut store_mut, args_sig.as_ref(), values)
                    {
                        Ok(args) => args,
                        Err(err) => return Box::pin(async { Err(err) }),
                    };
                drop(store_mut_wrapper);
                let future = func.as_ref().call_async(AsyncFunctionEnv::new(), args);
                Box::pin(async move {
                    let typed_result = future.await?;
                    let mut store_mut = env_mut.write().await;
                    typed_results_to_values::<Rets>(
                        &mut store_mut.as_store_mut(),
                        results_sig.as_ref(),
                        typed_result,
                    )
                })
            },
        )
    }

    #[cfg(feature = "experimental-async")]
    pub(crate) fn new_typed_with_env_async<T, F, Args, Rets>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        func: F,
    ) -> Self
    where
        T: 'static,
        F: AsyncHostFunction<T, Args, Rets, WithEnv> + 'static,
        Args: WasmTypeList + 'static,
        Rets: WasmTypeList + 'static,
    {
        let signature = FunctionType::new(Args::wasm_types(), Rets::wasm_types());
        let args_sig = Arc::new(signature.clone());
        let results_sig = Arc::new(signature.clone());
        let func = Arc::new(func);
        Self::new_with_env_async_boxed(
            store,
            env,
            signature,
            move |mut env_mut,
                  values|
                  -> Pin<Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>>>> {
                let sys_env = match env_mut.0 {
                    BackendAsyncFunctionEnvMut::Sys(ref mut sys_env) => sys_env,
                    _ => panic!("Not a sys backend"),
                };
                let mut store_mut_wrapper =
                    unsafe { StoreContext::get_current(sys_env.store_id()) };
                let mut store_mut = store_mut_wrapper.as_mut();
                let args_sig = args_sig.clone();
                let results_sig = results_sig.clone();
                let func = func.clone();
                let args =
                    match typed_args_from_values::<Args>(&mut store_mut, args_sig.as_ref(), values)
                    {
                        Ok(args) => args,
                        Err(err) => return Box::pin(async { Err(err) }),
                    };
                drop(store_mut_wrapper);
                let env_mut_clone = env_mut.as_mut();
                let future = func
                    .as_ref()
                    .call_async(AsyncFunctionEnv::with_env(env_mut), args);
                Box::pin(async move {
                    let typed_result = future.await?;
                    let mut store_mut = env_mut_clone.write().await;
                    typed_results_to_values::<Rets>(
                        &mut store_mut.as_store_mut(),
                        results_sig.as_ref(),
                        typed_result,
                    )
                })
            },
        )
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
        let func_ptr = func.function_callback_sys().unwrap_sys();
        let host_data = Box::new(StaticFunction {
            store_id: store.objects_mut().id(),
            slot: HostSlot::new(SlotRef::of(store.objects_mut())),
            env: env.as_sys().clone().into(),
            func,
        });
        let function_type = FunctionType::new(Args::wasm_types(), Rets::wasm_types());

        let type_signature_hash = store
            .as_store_ref()
            .engine()
            .as_sys()
            .register_signature(&function_type);
        let vmctx = VMFunctionContext {
            host_env: host_data.as_ref() as *const _ as *mut c_void,
        };
        let call_trampoline =
            <F as HostFunction<T, Args, Rets, WithEnv>>::call_trampoline_address().unwrap_sys();
        let anyfunc = VMCallerCheckedAnyfunc {
            include_m0_param: false,
            func_ptr,
            type_signature_hash,
            vmctx,
            call_trampoline,
        };

        let vm_function = VMFunction {
            anyfunc: MaybeInstanceOwned::Host(Box::new(UnsafeCell::new(anyfunc))),
            kind: VMFunctionKind::Static,
            signature: function_type,
            host_data: HostFunctionData::boxed(host_data, |data| &data.slot, true),
        };
        Self {
            handle: StoreHandle::new(store.objects_mut().as_sys_mut(), vm_function),
        }
    }

    pub(crate) fn ty(&self, store: &impl AsStoreRef) -> FunctionType {
        self.handle
            .get(store.as_store_ref().objects().as_sys())
            .signature
            .clone()
    }

    /// Calls the function with its arguments in `params`, which must also have
    /// room for its results: the call leaves them at the start of `params`.
    fn call_wasm_raw(
        &self,
        store: &mut impl AsStoreMut,
        wrapper: Option<&mut dyn CallWrapper>,
        trampoline: VMTrampoline,
        params: &mut [RawValue],
    ) -> Result<(), RuntimeError> {
        // Call the trampoline.
        let result = {
            #[cfg(feature = "experimental-host-interrupt")]
            let interrupt_guard = match interrupt_registry::install(store.objects_mut().id()) {
                Ok(x) => x,
                Err(interrupt_registry::InstallError::AlreadyInterrupted) => {
                    return Err(Trap::lib(TrapCode::HostInterrupt).into());
                }
            };

            // The trampoline writes the results over the arguments, so a
            // wrapper that enters again gets them restored first.
            let original_params = wrapper.is_some().then(|| RawValues::from_slice(params));

            let r = call_wrapped(store, wrapper, |store| {
                if let Some(original_params) = &original_params {
                    params.copy_from_slice(original_params);
                }
                // Safety: the store context is uninstalled before this entry
                // returns, and the store mut is valid for its duration. It is
                // installed only while Wasm runs: the wrapper holds the store
                // between entries, so it must not be lendable then. One lookup
                // of the thread serves for marking the call and for entering.
                let thread = wasmer_vm::CurrentThread::get();
                let store_install_guard = unsafe {
                    StoreContext::enter_on(thread, store.as_store_mut().inner as *mut _)
                };
                let storeref = store.as_store_ref();
                let vm_function = self.handle.get(storeref.objects().as_sys());
                let config = storeref.engine().tunables().vmconfig();
                let signal_handler = storeref.signal_handler();
                unsafe {
                    // Safety: This is the intended use-case for StoreContext::pause, as
                    // documented in the function's doc comments.
                    let _pause_guard = store_install_guard.pause();
                    wasmer_vm::wasmer_call_trampoline_on(
                        thread,
                        signal_handler,
                        config,
                        vm_function.anyfunc.as_ptr().as_ref().vmctx,
                        trampoline,
                        vm_function.anyfunc.as_ptr().as_ref().func_ptr,
                        params.as_mut_ptr() as *mut u8,
                    )
                }
                .map_err(RuntimeError::from)
            });

            #[cfg(feature = "experimental-host-interrupt")]
            drop(interrupt_guard);

            r
        };

        result?;
        Ok(())
    }

    pub(crate) fn result_arity(&self, store: &impl AsStoreRef) -> usize {
        self.handle
            .get(store.as_store_ref().objects().as_sys())
            .signature
            .results()
            .len()
    }

    pub(crate) fn call(
        &self,
        store: &mut impl AsStoreMut,
        params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        self.call_wrapped(store, None, params)
    }

    pub(crate) fn call_wrapped(
        &self,
        store: &mut impl AsStoreMut,
        wrapper: Option<&mut dyn CallWrapper>,
        params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        let store_ref = store.as_store_ref();
        let vm_function = self.handle.get(store_ref.objects().as_sys());
        let signature = &vm_function.signature;
        let trampoline = unsafe { vm_function.anyfunc.as_ptr().as_ref().call_trampoline };
        let format_types_for_error_message = |items: &[Value]| {
            items
                .iter()
                .map(|param| param.ty().to_string())
                .collect::<Vec<String>>()
                .join(", ")
        };
        if signature.params().len() != params.len() {
            return Err(RuntimeError::new(format!(
                "Parameters of type [{}] did not match signature {}",
                format_types_for_error_message(params),
                signature
            )));
        }

        let mut values_vec: RawValues =
            smallvec![RawValue { i32: 0 }; max(params.len(), signature.results().len())];

        // Store the argument values into `values_vec`.
        for ((arg, slot), ty) in params
            .iter()
            .zip(&mut values_vec)
            .zip(signature.params())
        {
            if arg.ty() != *ty {
                let param_types = format_types_for_error_message(params);
                return Err(RuntimeError::new(format!(
                    "Parameters of type [{param_types}] did not match signature {signature}",
                )));
            }
            if !arg.is_from_store(&store_ref) {
                return Err(RuntimeError::new("cross-`Store` values are not supported"));
            }
            *slot = arg.as_raw(&store_ref);
        }

        // The result types are copied out, and the borrow of the function
        // ends here: its signature lives in the store's function list, which
        // the call (or reading a funcref result) may grow and move.
        let result_types = Types::from_slice(signature.results());

        self.call_wasm_raw(store, wrapper, trampoline, &mut values_vec)?;
        Ok(unsafe { results_from_raw(store, &result_types, &values_vec) })
    }

    #[cfg(feature = "experimental-async")]
    #[allow(clippy::type_complexity)]
    pub(crate) fn call_async(
        &self,
        store: &impl AsStoreAsync,
        params: Vec<Value>,
    ) -> Pin<Box<dyn Future<Output = Result<Box<[Value]>, RuntimeError>> + 'static>> {
        let function = self.clone();
        let store = store.store();
        Box::pin(call_function_async(function, store, params))
    }

    #[doc(hidden)]
    #[allow(missing_docs)]
    pub(crate) fn call_raw(
        &self,
        store: &mut impl AsStoreMut,
        mut params: Vec<RawValue>,
    ) -> Result<Box<[Value]>, RuntimeError> {
        let (trampoline, result_types) = {
            let vm_function = self.handle.get(store.objects_mut().as_sys());
            let trampoline = unsafe { vm_function.anyfunc.as_ptr().as_ref().call_trampoline };
            let result_types = Types::from_slice(vm_function.signature.results());
            (trampoline, result_types)
        };
        self.call_wasm_raw(store, None, trampoline, &mut params)?;
        Ok(unsafe { results_from_raw(store, &result_types, &params) })
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
                .as_store_mut()
                .engine()
                .as_sys()
                .lookup_signature(anyfunc.type_signature_hash)
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
                StoreHandle::from_internal(store.objects_mut().id(), vm_extern.unwrap_sys())
            },
        }
    }

    /// Checks whether this `Function` can be used with the given store.
    pub(crate) fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.handle.store_id() == store.as_store_ref().objects().id()
    }

    /// See [`crate::Function::set_calls_on_native_stack`].
    pub(crate) fn set_calls_on_native_stack(&self, store: &mut impl AsStoreMut, always: bool) {
        let objects = store.objects_mut().as_sys_mut();
        if let Some(data) = self.handle.get(objects).host_data.downcast_ref::<HostFunctionData>() {
            data.slot().set_on_native_stack(always);
        }
    }

    /// See [`crate::Function::calls_on_native_stack`].
    pub(crate) fn calls_on_native_stack(&self, store: &impl AsStoreRef) -> bool {
        let store = store.as_store_ref();
        let function = self.handle.get(store.objects().as_sys());
        function
            .host_data
            .downcast_ref::<HostFunctionData>()
            .is_some_and(|data| !data.in_place || data.slot().get().1)
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
//
// The outcomes that still need the store on the Wasm stack carry the pointer
// the host function borrowed it through: it is the active entry's, as nothing
// has run since, so the trampoline does not have to look it up again.
enum InvocationResult<T, E> {
    Success(T, *mut StoreInner),
    Exception(crate::Exception, *mut StoreInner),
    Trap(Box<E>),
    YieldOutsideAsyncContext,
}

/// `store` gives the pointer for the outcomes that carry one, and is not
/// called for the others.
fn to_invocation_result<T, E>(
    result: Result<T, E>,
    store: impl FnOnce() -> *mut StoreInner,
) -> InvocationResult<T, E>
where
    E: Error + 'static,
{
    match result {
        Ok(value) => InvocationResult::Success(value, store()),
        Err(trap) => {
            let dyn_err_ref = &trap as &dyn Error;
            if let Some(runtime_error) = dyn_err_ref.downcast_ref::<RuntimeError>()
                && let Some(exception) = runtime_error.to_exception()
            {
                return InvocationResult::Exception(exception, store());
            }
            InvocationResult::Trap(Box::new(trap))
        }
    }
}

/// Reads the results a call left at the start of `raw`.
///
/// # Safety
///
/// `raw` must start with values of `types`, as a call to a function with
/// these result types leaves them.
unsafe fn results_from_raw(
    store: &mut impl AsStoreMut,
    types: &[Type],
    raw: &[RawValue],
) -> Box<[Value]> {
    let mut values = Vec::with_capacity(types.len());
    for (&ty, &raw) in types.iter().zip(&raw[..types.len()]) {
        values.push(unsafe { Value::from_raw(store, ty, raw) });
    }
    values.into_boxed_slice()
}

fn write_dynamic_results(
    store_wrapper: &mut StorePtrWrapper,
    func_ty: &FunctionType,
    returns: Vec<Value>,
    values_vec: *mut RawValue,
) -> Result<(), RuntimeError> {
    let store = store_wrapper.as_mut();
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

fn typed_args_from_values<Args>(
    store: &mut StoreMut,
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
        *slot = value.as_raw(store);
    }
    unsafe { Ok(Args::from_array(store, raw_array)) }
}

fn typed_results_to_values<Rets>(
    store: &mut StoreMut,
    func_ty: &FunctionType,
    rets: Rets,
) -> Result<Vec<Value>, RuntimeError>
where
    Rets: WasmTypeList,
{
    let mut raw_array = unsafe { rets.into_array(store) };
    let mut values = Vec::with_capacity(func_ty.results().len());
    for (raw, ty) in raw_array.as_mut().iter().zip(func_ty.results().iter()) {
        unsafe {
            values.push(Value::from_raw(store, *ty, *raw));
        }
    }
    Ok(values)
}

pub(crate) enum HostCallOutcome {
    /// The host function's results, and the borrow of the store it ran with,
    /// which writing the results back reuses.
    Ready(Result<Vec<Value>, RuntimeError>, StorePtrWrapper),
    /// The host function's future, and the entry it borrows a plain store
    /// through, which is removed once the future is done with.
    #[cfg(feature = "experimental-async")]
    Future(HostFuture, Option<StoreInstallGuard>),
}

/// Host state for a dynamic function.
pub(crate) struct DynamicFunction<F> {
    func: F,
    store_id: StoreId,
    /// The store's slot, where a call into a plain store leaves it, and
    /// whether every call runs on the native stack all the same.
    slot: HostSlot,
    /// The function's type, which its results are checked against.
    func_ty: FunctionType,
    /// Whether a call from Wasm may run in place, on the Wasm stack (see
    /// `wasmer_vm::host_call_in_place`): false for asynchronous functions.
    in_place: bool,
}

impl<F> DynamicFunction<F>
where
    F: Fn(*mut RawValue) -> HostCallOutcome + 'static,
{
    // This function wraps our func, to make it compatible with the
    // reverse trampoline signature.
    //
    // With `IN_PLACE`, a call runs on the Wasm stack it came from when enough
    // of it is left, and otherwise goes to the other instance, which switches
    // to the host stack. That instance stays out of line, and with it the
    // registers the switch makes every caller save.
    #[inline(never)]
    unsafe extern "C-unwind" fn func_wrapper<const IN_PLACE: bool>(
        this: &mut VMDynamicFunctionContext<Self>,
        values_vec: *mut RawValue,
    ) {
        let call = || {
            panic::catch_unwind(AssertUnwindSafe(|| match (this.ctx.func)(values_vec) {
                HostCallOutcome::Ready(result, mut store_wrapper) => {
                    let store = store_wrapper.as_ptr();
                    let result = result.and_then(|values| {
                        write_dynamic_results(
                            &mut store_wrapper,
                            &this.ctx.func_ty,
                            values,
                            values_vec,
                        )
                    });
                    to_invocation_result(result, || store)
                }
                // The host function gave its borrow back before the future
                // was polled, which may lend the store out: it is borrowed
                // again once the future is done.
                #[cfg(feature = "experimental-async")]
                HostCallOutcome::Future(future, plain_store_entry) => {
                    // A call into a plain store never suspends: its caller
                    // holds the store across the call. Only a call through
                    // the context stack may be a `call_async` coroutine's.
                    let awaited = if plain_store_entry.is_some() {
                        run_immediate(future)
                    } else {
                        block_on_host_future(future)
                    };
                    let result = match awaited {
                        Ok(value) => Ok(value),
                        Err(AsyncRuntimeError::RuntimeError(e)) => Err(e),
                        Err(AsyncRuntimeError::YieldOutsideAsyncContext) => {
                            return InvocationResult::YieldOutsideAsyncContext;
                        }
                    };
                    let result = result.and_then(|values| {
                        let mut store_wrapper = unsafe {
                            StoreContext::get_current_in(this.ctx.slot.slot(), this.ctx.store_id)
                        };
                        write_dynamic_results(
                            &mut store_wrapper,
                            &this.ctx.func_ty,
                            values,
                            values_vec,
                        )
                    });
                    let result = to_invocation_result(result, || unsafe {
                        StoreContext::get_current_transient_in(this.ctx.slot.slot(), this.ctx.store_id)
                    });
                    drop(plain_store_entry);
                    result
                }
            }))
        };
        let result = if IN_PLACE {
            let (slot, on_native_stack) = this.ctx.slot.get();
            // A call into a plain store left its thread in the slot, which
            // saves looking the thread up.
            let thread = slot
                .thread()
                .unwrap_or_else(wasmer_vm::CurrentThread::get_out_of_line);
            // PROTOTYPE (fast-wasmer): not for a guest on the thread's own
            // stack, which has no other stack (see below).
            if on_native_stack && !thread.on_thread_stack() {
                return unsafe { Self::func_wrapper::<false>(this, values_vec) };
            }
            match wasmer_vm::host_call_in_place_on(thread) {
                Some(in_place) => {
                    let result = call();
                    drop(in_place);
                    result
                }
                None => return unsafe { Self::func_wrapper::<false>(this, values_vec) },
            }
        } else {
            let thread = this
                .ctx
                .slot
                .slot()
                .thread()
                .unwrap_or_else(wasmer_vm::CurrentThread::get_out_of_line);
            // PROTOTYPE (fast-wasmer): a guest on the thread's own stack has
            // no other stack, and its host functions run in place whatever
            // they are set to. An asynchronous function never gets here from
            // such a guest with a coroutine to suspend: `call_async` runs its
            // guests on coroutine stacks.
            if thread.on_thread_stack() {
                return unsafe { Self::func_wrapper::<true>(this, values_vec) };
            }
            wasmer_vm::on_host_stack_on(thread, call)
        };

        // IMPORTANT: DO NOT ALLOCATE ON THE STACK: this runs on the Wasm stack,
        // and when the call ran on the host stack, little of the Wasm stack
        // may be left. See: https://github.com/wasmerio/wasmer/pull/5700
        match result {
            Ok(InvocationResult::Success((), store_ptr)) => unsafe {
                // Note: can't acquire a proper ref-counted context ref here, since we can switch
                // away from the WASM stack at any time.
                // Safety: The pointer is only used for the duration of the call to
                // `is_interrupted`.
                let mut store = store_ptr.as_mut().unwrap();
                #[cfg(feature = "experimental-host-interrupt")]
                if interrupt_registry::is_interrupted(store.objects.id()) {
                    raise_lib_trap(Trap::lib(TrapCode::HostInterrupt))
                }
            },
            Ok(InvocationResult::Exception(exception, store_ptr)) => unsafe {
                // Note: can't acquire a proper ref-counted context ref here, since we can switch
                // away from the WASM stack at any time.
                // Safety: The pointer is only used for the duration of the call to `throw` and
                // `is_interrupted`.
                let mut store = store_ptr.as_mut().unwrap();
                #[cfg(feature = "experimental-host-interrupt")]
                if interrupt_registry::is_interrupted(store.objects.id()) {
                    raise_lib_trap(Trap::lib(TrapCode::HostInterrupt))
                }
                wasmer_vm::libcalls::throw(
                    store.objects.as_sys(),
                    exception.vm_exceptionref().unwrap_sys_ref().to_u32_exnref(),
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
        if self.in_place {
            Self::func_wrapper::<true> as VMFunctionCallback
        } else {
            Self::func_wrapper::<false> as VMFunctionCallback
        }
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
            if dynamic_function.ctx.in_place {
                Self::func_wrapper::<true>(dynamic_function, args);
            } else {
                Self::func_wrapper::<false>(dynamic_function, args);
            }
        }
    }
}

/// Represents a low-level Wasm static host function. See
/// [`crate::Function::new_typed`] and
/// [`crate::Function::new_typed_with_env`] to learn more.
pub(crate) struct StaticFunction<F, T> {
    pub(crate) store_id: StoreId,
    /// The store's slot, where a call into a plain store leaves it, and
    /// whether every call runs on the native stack all the same.
    pub(crate) slot: HostSlot,
    pub(crate) env: FunctionEnv<T>,
    pub(crate) func: F,
}

/// The slot of a host function's store (see [`SlotRef`]), with the setting
/// that [`crate::Function::set_calls_on_native_stack`] writes in the lowest
/// bit of its address, which the alignment of a slot leaves free. A call
/// reads the setting with the slot it loads anyway, so the setting costs it
/// no memory access.
pub(crate) struct HostSlot(AtomicPtr<wasmer_vm::StoreSlot>);

impl HostSlot {
    /// The bit of the address that says calls always run on the native stack.
    const ON_NATIVE_STACK: usize = 1;

    fn new(slot: SlotRef) -> Self {
        const {
            assert!(std::mem::align_of::<wasmer_vm::StoreSlot>() > Self::ON_NATIVE_STACK);
        }
        Self(AtomicPtr::new(slot.as_ptr().as_ptr()))
    }

    /// The slot, and whether every call runs on the native stack.
    #[inline(always)]
    fn get(&self) -> (SlotRef, bool) {
        let tagged = self.0.load(Ordering::Relaxed);
        let slot = tagged.map_addr(|addr| addr & !Self::ON_NATIVE_STACK);
        // SAFETY: `new` stored the address of a slot, and only the bit
        // changes after that.
        let slot = unsafe { SlotRef::from_ptr(NonNull::new_unchecked(slot)) };
        (slot, tagged.addr() & Self::ON_NATIVE_STACK != 0)
    }

    /// The slot.
    #[inline(always)]
    fn slot(&self) -> SlotRef {
        self.get().0
    }

    fn set_on_native_stack(&self, always: bool) {
        // Only the thread that holds the store writes it, and nothing else
        // changes the address.
        let tagged = self.0.load(Ordering::Relaxed).map_addr(|addr| {
            if always {
                addr | Self::ON_NATIVE_STACK
            } else {
                addr & !Self::ON_NATIVE_STACK
            }
        });
        self.0.store(tagged, Ordering::Relaxed);
    }
}

/// What the `VMFunction` of a host function owns: the function's state, and
/// where in it the slot is that [`crate::Function::set_calls_on_native_stack`]
/// sets the bit of.
struct HostFunctionData {
    /// Inside `_state`, which keeps it in place.
    slot: NonNull<HostSlot>,
    /// Whether a call may run on the Wasm stack at all: false for
    /// asynchronous functions, which always run on the native stack.
    in_place: bool,
    _state: Box<dyn Any>,
}

impl HostFunctionData {
    fn boxed<S: 'static>(state: Box<S>, slot: fn(&S) -> &HostSlot, in_place: bool) -> Box<dyn Any> {
        Box::new(Self {
            slot: NonNull::from(slot(&state)),
            in_place,
            _state: state,
        })
    }

    fn slot(&self) -> &HostSlot {
        // SAFETY: the slot is inside `_state`, whose allocation lives as long
        // as `self` and does not move.
        unsafe { self.slot.as_ref() }
    }
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
            // With `IN_PLACE`, a call runs on the Wasm stack it came from when
            // enough of it is left, and otherwise goes to the other instance,
            // which switches to the host stack. That instance stays out of line,
            // and with it the registers the switch makes every caller save.
            #[inline(never)]
            unsafe extern "C-unwind" fn func_wrapper<const IN_PLACE: bool, $( $x, )* Rets, RetsAsResult, Func>( env: &StaticFunction<Func, ()>, $( $x: <$x::Native as NativeWasmType>::Abi, )* ) -> Rets::CStruct
            where
                $( $x: FromToNativeWasmType, )*
                Rets: WasmTypeList,
                RetsAsResult: IntoResult<Rets>,
                Func: Fn($( $x , )*) -> RetsAsResult + 'static,
            {
                let call = || {
                    panic::catch_unwind(AssertUnwindSafe(|| {
                        let mut store_wrapper = unsafe { StoreContext::get_current_in(env.slot.slot(), env.store_id) };
                        let store_ptr = store_wrapper.as_ptr();
                        let mut store = store_wrapper.as_mut();
                        $(
                            let $x = unsafe {
                                FromToNativeWasmType::from_native(NativeWasmTypeInto::from_abi(&mut store, $x))
                            };
                        )*
                        to_invocation_result((env.func)($($x),* ).into_result(), || store_ptr)
                    }))
                };
                let result = if IN_PLACE {
                    // A call into a plain store left its thread in the slot,
                    // which saves looking the thread up.
                    let (slot, on_native_stack) = env.slot.get();
                    let thread = slot.thread().unwrap_or_else(wasmer_vm::CurrentThread::get_out_of_line);
                    // rvw2: a function set to the native stack runs in place
                    // only for a guest on the thread's own stack.
                    if on_native_stack && !thread.on_thread_stack() {
                        return unsafe { func_wrapper::<false, $( $x, )* Rets, RetsAsResult, Func>(env, $( $x ),*) };
                    }
                    match wasmer_vm::host_call_in_place_on(thread) {
                        Some(in_place) => {
                            let result = call();
                            drop(in_place);
                            result
                        }
                        None => return unsafe { func_wrapper::<false, $( $x, )* Rets, RetsAsResult, Func>(env, $( $x ),*) },
                    }
                } else {
                    // The thread from the slot here as well: the switch needs
                    // the thread's state, and nothing else of the thread.
                    let thread = env.slot.slot().thread().unwrap_or_else(wasmer_vm::CurrentThread::get_out_of_line);
                    // PROTOTYPE (fast-wasmer): a guest on the thread's own
                    // stack has no other stack, and its host functions run
                    // in place whatever they are set to.
                    if thread.on_thread_stack() {
                        return unsafe { func_wrapper::<true, $( $x, )* Rets, RetsAsResult, Func>(env, $( $x ),*) };
                    }
                    wasmer_vm::on_host_stack_on(thread, call)
                };

                // IMPORTANT: DO NOT ALLOCATE ON THE STACK: this runs on the Wasm
                // stack, and when the call ran on the host stack, little of the
                // Wasm stack may be left. See: https://github.com/wasmerio/wasmer/pull/5700
                match result {
                    Ok(InvocationResult::Success(result, store_ptr)) => unsafe {
                        // Note: can't acquire a proper ref-counted context ref here, since we can switch
                        // away from the WASM stack at any time.
                        // Safety: The pointer is only used for the duration of the call to
                        // `into_c_struct` and `is_interrupted`.
                        let mut store = store_ptr.as_mut().unwrap();
                        #[cfg(feature = "experimental-host-interrupt")]
                        if interrupt_registry::is_interrupted(store.objects.id()) {
                            raise_lib_trap(Trap::lib(TrapCode::HostInterrupt))
                        }
                        return result.into_c_struct(store);
                    },
                    Ok(InvocationResult::Exception(exception, store_ptr)) => unsafe {
                        // Note: can't acquire a proper ref-counted context ref here, since we can switch
                        // away from the WASM stack at any time.
                        // Safety: The pointer is only used for the duration of the call to `throw` and
                        // `is_interrupted`.
                        let mut store = store_ptr.as_mut().unwrap();
                        #[cfg(feature = "experimental-host-interrupt")]
                        if interrupt_registry::is_interrupted(store.objects.id()) {
                            raise_lib_trap(Trap::lib(TrapCode::HostInterrupt))
                        }
                        wasmer_vm::libcalls::throw(
                            store.objects.as_sys(),
                            exception.vm_exceptionref().unwrap_sys_ref().to_u32_exnref(),
                        )
                    }
                    Ok(InvocationResult::Trap(trap)) => unsafe { raise_user_trap(trap) },
                    Ok(InvocationResult::YieldOutsideAsyncContext) => unsafe {
                        raise_lib_trap(Trap::lib(TrapCode::YieldOutsideAsyncContext))
                    },
                    Err(panic) => unsafe { resume_panic(panic) },
                }
            }

            /// What Wasm calls: picks the instance of the wrapper by the
            /// setting of `Function::set_calls_on_native_stack`, with a jump
            /// and no frame of its own, so a function that always runs on the
            /// native stack does not set up the frame of the other instance.
            #[allow(dead_code)]
            unsafe extern "C-unwind" fn dispatch<$( $x, )* Rets, RetsAsResult, Func>( env: &StaticFunction<Func, ()>, $( $x: <$x::Native as NativeWasmType>::Abi, )* ) -> Rets::CStruct
            where
                $( $x: FromToNativeWasmType, )*
                Rets: WasmTypeList,
                RetsAsResult: IntoResult<Rets>,
                Func: Fn($( $x , )*) -> RetsAsResult + 'static,
            {
                if env.slot.get().1 {
                    unsafe { func_wrapper::<false, $( $x, )* Rets, RetsAsResult, Func>(env, $( $x ),*) }
                } else {
                    unsafe { func_wrapper::<true, $( $x, )* Rets, RetsAsResult, Func>(env, $( $x ),*) }
                }
            }

            func_wrapper::< true, $( $x, )* Rets, RetsAsResult, Func > as _

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
            // With `IN_PLACE`, a call runs on the Wasm stack it came from when
            // enough of it is left, and otherwise goes to the other instance,
            // which switches to the host stack. That instance stays out of line,
            // and with it the registers the switch makes every caller save.
            #[inline(never)]
            unsafe extern "C-unwind" fn func_wrapper<const IN_PLACE: bool, T: Send + 'static, $( $x, )* Rets, RetsAsResult, Func>( env: &StaticFunction<Func, T>, $( $x: <$x::Native as NativeWasmType>::Abi, )* ) -> Rets::CStruct
                where
                $( $x: FromToNativeWasmType, )*
                Rets: WasmTypeList,
                RetsAsResult: IntoResult<Rets>,
                Func: Fn(FunctionEnvMut<T>, $( $x , )*) -> RetsAsResult + 'static,
            {
                let call = || {
                    panic::catch_unwind(AssertUnwindSafe(|| {
                        let mut store_wrapper = unsafe { StoreContext::get_current_in(env.slot.slot(), env.store_id) };
                        let store_ptr = store_wrapper.as_ptr();
                        let mut store = store_wrapper.as_mut();
                        $(
                            let $x = unsafe {
                                FromToNativeWasmType::from_native(NativeWasmTypeInto::from_abi(&mut store, $x))
                            };
                        )*
                        let f_env = crate::backend::sys::function::env::FunctionEnvMut {
                            store_mut: store,
                            func_env: env.env.as_sys().clone(),
                        }.into();
                        to_invocation_result((env.func)(f_env, $($x),* ).into_result(), || store_ptr)
                    }))
                };
                let result = if IN_PLACE {
                    // A call into a plain store left its thread in the slot,
                    // which saves looking the thread up.
                    let (slot, on_native_stack) = env.slot.get();
                    let thread = slot.thread().unwrap_or_else(wasmer_vm::CurrentThread::get_out_of_line);
                    // rvw2: a function set to the native stack runs in place
                    // only for a guest on the thread's own stack.
                    if on_native_stack && !thread.on_thread_stack() {
                        return unsafe { func_wrapper::<false, T, $( $x, )* Rets, RetsAsResult, Func>(env, $( $x ),*) };
                    }
                    match wasmer_vm::host_call_in_place_on(thread) {
                        Some(in_place) => {
                            let result = call();
                            drop(in_place);
                            result
                        }
                        None => return unsafe { func_wrapper::<false, T, $( $x, )* Rets, RetsAsResult, Func>(env, $( $x ),*) },
                    }
                } else {
                    // The thread from the slot here as well: the switch needs
                    // the thread's state, and nothing else of the thread.
                    let thread = env.slot.slot().thread().unwrap_or_else(wasmer_vm::CurrentThread::get_out_of_line);
                    // PROTOTYPE (fast-wasmer): a guest on the thread's own
                    // stack has no other stack, and its host functions run
                    // in place whatever they are set to.
                    if thread.on_thread_stack() {
                        return unsafe { func_wrapper::<true, T, $( $x, )* Rets, RetsAsResult, Func>(env, $( $x ),*) };
                    }
                    wasmer_vm::on_host_stack_on(thread, call)
                };

                // IMPORTANT: DO NOT ALLOCATE ON THE STACK: this runs on the Wasm
                // stack, and when the call ran on the host stack, little of the
                // Wasm stack may be left. See: https://github.com/wasmerio/wasmer/pull/5700
                match result {
                    Ok(InvocationResult::Success(result, store_ptr)) => unsafe {
                        // Note: can't acquire a proper ref-counted context ref here, since we can switch
                        // away from the WASM stack at any time.
                        // Safety: The pointer is only used for the duration of the call to
                        // `into_c_struct` and `is_interrupted`.
                        let mut store = store_ptr.as_mut().unwrap();
                        #[cfg(feature = "experimental-host-interrupt")]
                        if interrupt_registry::is_interrupted(store.objects.id()) {
                            raise_lib_trap(Trap::lib(TrapCode::HostInterrupt))
                        }
                        return result.into_c_struct(store);
                    },
                    Ok(InvocationResult::Exception(exception, store_ptr)) => unsafe {
                        // Note: can't acquire a proper ref-counted context ref here, since we can switch
                        // away from the WASM stack at any time.
                        // Safety: The pointer is only used for the duration of the call to `throw` and
                        // `is_interrupted`.
                        let mut store = store_ptr.as_mut().unwrap();
                        #[cfg(feature = "experimental-host-interrupt")]
                        if interrupt_registry::is_interrupted(store.objects.id()) {
                            raise_lib_trap(Trap::lib(TrapCode::HostInterrupt))
                        }
                        wasmer_vm::libcalls::throw(
                            store.objects.as_sys(),
                            exception.vm_exceptionref().unwrap_sys_ref().to_u32_exnref(),
                        )
                    }
                    Ok(InvocationResult::Trap(trap)) => unsafe { raise_user_trap(trap) },
                    Ok(InvocationResult::YieldOutsideAsyncContext) => unsafe {
                        raise_lib_trap(Trap::lib(TrapCode::YieldOutsideAsyncContext))
                    },
                    Err(panic) => unsafe { resume_panic(panic) },
                }
            }
            /// What Wasm calls: picks the instance of the wrapper by the
            /// setting of `Function::set_calls_on_native_stack`, with a jump
            /// and no frame of its own, so a function that always runs on the
            /// native stack does not set up the frame of the other instance.
            #[allow(dead_code)]
            unsafe extern "C-unwind" fn dispatch<T: Send + 'static, $( $x, )* Rets, RetsAsResult, Func>( env: &StaticFunction<Func, T>, $( $x: <$x::Native as NativeWasmType>::Abi, )* ) -> Rets::CStruct
            where
                $( $x: FromToNativeWasmType, )*
                Rets: WasmTypeList,
                RetsAsResult: IntoResult<Rets>,
                Func: Fn(FunctionEnvMut<T>, $( $x , )*) -> RetsAsResult + 'static,
            {
                if env.slot.get().1 {
                    unsafe { func_wrapper::<false, T, $( $x, )* Rets, RetsAsResult, Func>(env, $( $x ),*) }
                } else {
                    unsafe { func_wrapper::<true, T, $( $x, )* Rets, RetsAsResult, Func>(env, $( $x ),*) }
                }
            }

            func_wrapper::< true, T, $( $x, )* Rets, RetsAsResult, Func > as _
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
