//! Drive a typed V8 continuation on the store thread.
use crate::{
    AsStoreAsync, AsStoreMut, AsStoreRef, AsyncFunctionEnv, AsyncFunctionEnvMut, AsyncHostFunction,
    BackendAsyncFunctionEnvMut, BackendTrap, FunctionEnv, RuntimeError, StoreAsync, StoreContext,
    StoreMut, Value, WasmTypeList, WithEnv, WithoutEnv,
    v8::{
        bindings::{self, *},
        error::Trap,
        function::{Function, env},
        utils::convert::*,
    },
};
use futures::FutureExt;
use std::{
    cell::RefCell,
    ffi::c_void,
    future::Future,
    panic::{AssertUnwindSafe, catch_unwind},
    pin::Pin,
    ptr,
    rc::Rc,
};
use wasmer_types::{FunctionType, StoreId, TrapCode, Type};

type Values = Result<Vec<Value>, RuntimeError>;
type LocalFuture<T> = Pin<Box<dyn Future<Output = T>>>;
type HostFuture = LocalFuture<Values>;
type HostCallback = dyn Fn(&[Value]) -> HostFuture;

struct Callback {
    store: StoreId,
    results: Box<[Type]>,
    function: Box<HostCallback>,
}

struct Turn {
    store: StoreId,
    pending: RefCell<Option<HostFuture>>,
}

thread_local! {
    static ACTIVE: RefCell<Option<Rc<Turn>>> = const { RefCell::new(None) };
}

impl Turn {
    fn current(id: StoreId) -> Option<Rc<Self>> {
        ACTIVE.with_borrow(|current| current.as_ref().filter(|turn| turn.store == id).cloned())
    }

    fn enter<T>(self: &Rc<Self>, f: impl FnOnce() -> T) -> T {
        with_turn(Some(self.clone()), f)
    }
}

pub(crate) fn enter_sync<T>(store: &mut StoreMut, f: impl FnOnce() -> T) -> T {
    with_turn(None, || store.parked(f))
}

fn with_turn<T>(turn: Option<Rc<Turn>>, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<Rc<Turn>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            ACTIVE.replace(self.0.take());
        }
    }
    let _restore = Restore(ACTIVE.replace(turn));
    f()
}

struct Continuation(*mut wasm_pending_call_t);
impl Drop for Continuation {
    fn drop(&mut self) {
        unsafe { wasm_pending_call_delete(self.0) }
    }
}

// The C API lends reference arguments until the callback returns. Keep
// a separate owned reference if Rust retains a function in a future.
unsafe fn read_value(store: StoreId, val: wasm_val_t) -> Result<Value, RuntimeError> {
    match val.kind as wasm_valkind_enum {
        bindings::wasm_valkind_enum_WASM_EXTERNREF if unsafe { val.of.ref_ }.is_null() => {
            Ok(Value::ExternRef(None))
        }
        bindings::wasm_valkind_enum_WASM_EXTERNREF => Err(RuntimeError::new(
            "V8 does not support non-null externref host values",
        )),
        _ => Ok(val.borrow_wv(store)),
    }
}

fn write_value(store: StoreId, value: Value) -> Result<wasm_val_t, RuntimeError> {
    match value {
        Value::ExternRef(None) => Ok(wasm_val_t {
            kind: wasm_valkind_enum_WASM_EXTERNREF as _,
            of: wasm_val_t__bindgen_ty_1 {
                ref_: ptr::null_mut(),
            },
        }),
        Value::FuncRef(Some(func)) => {
            let current = unsafe { StoreContext::get_current(store) };
            if !func.is_from_store(&current.as_ref()) {
                return Err(RuntimeError::new(
                    "function reference belongs to another store",
                ));
            }
            let handle = unsafe { wasm_func_as_ref(wasm_func_copy(func.as_v8().handle)) };
            Ok(wasm_val_t {
                kind: wasm_valkind_enum_WASM_FUNCREF as _,
                of: wasm_val_t__bindgen_ty_1 { ref_: handle },
            })
        }
        Value::ExternRef(Some(_)) | Value::ExceptionRef(_) => {
            Err(RuntimeError::new("unsupported V8 host reference value"))
        }
        value => Ok(value.into_cv()),
    }
}

struct OwnedValues(Vec<wasm_val_t>);

impl OwnedValues {
    fn from_values(store: StoreId, values: Vec<Value>) -> Result<Self, RuntimeError> {
        let mut owned = Self(Vec::with_capacity(values.len()));
        for value in values {
            owned.0.push(write_value(store, value)?);
        }
        Ok(owned)
    }

    fn vector(&mut self) -> wasm_val_vec_t {
        wasm_val_vec_t {
            size: self.0.len(),
            data: self.0.as_mut_ptr(),
        }
    }
}

impl Drop for OwnedValues {
    fn drop(&mut self) {
        for value in &mut self.0 {
            unsafe { wasm_val_delete(value) }
        }
    }
}

impl Callback {
    unsafe fn invoke(
        &self,
        args: &wasm_val_vec_t,
        results: &mut wasm_val_vec_t,
        pending: *mut bool,
    ) -> Result<(), RuntimeError> {
        let values = (0..args.size)
            .map(|i| unsafe { read_value(self.store, *args.data.add(i)) })
            .collect::<Result<Vec<_>, _>>()?;
        let future = (self.function)(&values);
        if !pending.is_null() {
            let turn = Turn::current(self.store)
                .ok_or_else(|| RuntimeError::new("missing async store context"))?;
            turn.pending.replace(Some(future));
            unsafe { *pending = true };
            return Ok(());
        }
        let returned = future.now_or_never().ok_or_else(|| {
            let code = TrapCode::YieldOutsideAsyncContext;
            RuntimeError::new_from_source(
                BackendTrap::V8(Trap::user(Box::new(code))),
                vec![],
                Some(code),
            )
        })??;
        if !returned
            .iter()
            .map(Value::ty)
            .eq(self.results.iter().copied())
        {
            return Err(RuntimeError::new(
                "V8 host result types do not match the signature",
            ));
        }
        for (i, value) in returned.into_iter().enumerate() {
            unsafe { results.data.add(i).write(write_value(self.store, value)?) };
        }
        Ok(())
    }
}

fn host(
    store: &mut StoreMut<'_>,
    ty: &FunctionType,
    function: impl Fn(&[Value]) -> HostFuture + 'static,
) -> *mut wasm_func_t {
    unsafe extern "C" fn invoke(
        env: *mut c_void,
        args: *const wasm_val_vec_t,
        results: *mut wasm_val_vec_t,
        pending: *mut bool,
    ) -> *mut wasm_trap_t {
        let callback = unsafe { &*env.cast::<Callback>() };
        // V8 finalizes the callback box. The C API owns both vectors for
        // this invocation, and sizes the output to the callback signature.
        let result = catch_unwind(AssertUnwindSafe(|| unsafe {
            callback.invoke(&*args, &mut *results, pending)
        }))
        .unwrap_or_else(|_| Err(RuntimeError::new("V8 async host callback panicked")));
        match result {
            Ok(()) => ptr::null_mut(),
            Err(error) => {
                let mut store = unsafe { StoreContext::get_current(callback.store) };
                unsafe { Trap::user(Box::new(error)).into_wasm_trap(&mut store.as_mut()) }
            }
        }
    }

    unsafe extern "C" fn unsupported_signature(
        store: *mut c_void,
        _: *const wasm_val_vec_t,
        _: *mut wasm_val_vec_t,
    ) -> *mut wasm_trap_t {
        let text = c"V8 JSPI does not support v128 parameters or results";
        let message = wasm_message_t {
            size: text.to_bytes_with_nul().len(),
            data: text.as_ptr().cast_mut().cast(),
        };
        unsafe { wasm_trap_new(store.cast(), &message) }
    }

    unsafe fn types(values: &[Type]) -> wasm_valtype_vec_t {
        let values: Vec<_> = values
            .iter()
            .map(|ty| unsafe { wasm_valtype_new(ty.into_ct()) })
            .collect();
        let mut output = wasm_valtype_vec_t::default();
        unsafe { wasm_valtype_vec_new(&mut output, values.len(), values.as_ptr()) };
        output
    }
    unsafe extern "C" fn drop_callback(data: *mut c_void) {
        drop(unsafe { Box::from_raw(data.cast::<Callback>()) });
    }

    let data = Box::into_raw(Box::new(Callback {
        store: store.objects_mut().id(),
        results: ty.results().into(),
        function: Box::new(function),
    }));
    let backend = store.inner.store.as_v8_mut();
    unsafe {
        let ty = wasm_functype_new(&mut types(ty.params()), &mut types(ty.results()));
        let func = wasm_func_new_suspending_with_env(
            backend.inner,
            ty,
            Some(invoke),
            data.cast(),
            Some(drop_callback),
        );
        // Wasmer host constructors are infallible. Keep the declared type so
        // linking succeeds and report an unsupported signature on invocation.
        let func = if func.is_null() {
            drop_callback(data.cast());
            wasm_func_new_with_env(
                backend.inner,
                ty,
                Some(unsupported_signature),
                backend.inner.cast(),
                None,
            )
        } else {
            func
        };
        wasm_functype_delete(ty);
        func
    }
}

impl Function {
    pub(crate) fn call_async(
        &self,
        store: &impl AsStoreAsync,
        params: Vec<Value>,
    ) -> LocalFuture<Result<Box<[Value]>, RuntimeError>> {
        let store = store.store();
        let function = self.clone();
        Box::pin(async move {
            let mut lock = store.write_lock().await;
            crate::v8::entities::check_isolate(&lock);
            if !function.is_from_store(&lock) {
                return Err(RuntimeError::new("function belongs to another store"));
            }
            let ty = function.ty(&lock);
            if !params.iter().map(Value::ty).eq(ty.params().iter().copied())
                || !params.iter().all(|v| v.is_from_store(&lock))
            {
                return Err(RuntimeError::new(
                    "V8 async call parameters do not match the function signature or store",
                ));
            }
            let turn = Rc::new(Turn {
                store: store.id,
                pending: RefCell::default(),
            });
            let mut continuation = Continuation(ptr::null_mut());
            let mut inputs = Ok(params);
            let mut output = OwnedValues(vec![wasm_val_t::default(); ty.results().len()]);
            loop {
                let context = StoreContext::install_async(lock.inner);
                let trap = turn.enter(|| -> Result<_, RuntimeError> {
                    let mut values = OwnedValues(Vec::new());
                    let error = match inputs {
                        Ok(args) => {
                            values = OwnedValues::from_values(store.id, args)?;
                            ptr::null_mut()
                        }
                        Err(error) => {
                            let mut current = unsafe { StoreContext::get_current(store.id) };
                            unsafe {
                                Trap::user(Box::new(error)).into_wasm_trap(&mut current.as_mut())
                            }
                        }
                    };
                    let trap = unsafe {
                        if continuation.0.is_null() {
                            let mut trap = ptr::null_mut();
                            let promising = wasm_func_promising(function.handle, &mut trap);
                            if promising.is_null() {
                                return Err(Trap::from(trap).into());
                            }
                            let trap = wasm_promising_func_call(
                                promising,
                                &values.vector(),
                                &mut output.vector(),
                                &mut continuation.0,
                            );
                            wasm_promising_func_delete(promising);
                            trap
                        } else {
                            wasm_pending_call_resume(
                                &mut continuation.0,
                                &values.vector(),
                                error,
                                &mut output.vector(),
                            )
                        }
                    };
                    if !error.is_null() {
                        unsafe { wasm_trap_delete(error) };
                    }
                    Ok(trap)
                })?;
                if !trap.is_null() {
                    return Err(Trap::from(trap).into());
                }
                if continuation.0.is_null() {
                    return output
                        .0
                        .iter()
                        .map(|v| unsafe { read_value(store.id, *v) })
                        .collect();
                }
                drop(context);
                let future = turn
                    .pending
                    .take()
                    .ok_or_else(|| RuntimeError::new("missing suspended V8 host future"))?;
                inputs = future.await;
                lock = store.write_lock().await;
            }
        })
    }

    pub(crate) fn new_async<FT, F, Fut>(store: &mut impl AsStoreMut, ty: FT, func: F) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(&[Value]) -> Fut + 'static,
        Fut: Future<Output = Values> + 'static,
    {
        let env = FunctionEnv::new(store, ());
        Self::new_with_env_async(store, &env, ty, move |_, values| func(values))
    }

    pub(crate) fn new_with_env_async<FT, F, Fut, T: 'static>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        ty: FT,
        func: F,
    ) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(AsyncFunctionEnvMut<T>, &[Value]) -> Fut + 'static,
        Fut: Future<Output = Values> + 'static,
    {
        crate::v8::entities::check_isolate(store);
        assert_eq!(
            env.as_v8().handle.store_id(),
            store.objects_mut().id(),
            "V8 host environment belongs to another store"
        );
        let mut store = store.as_store_mut();
        let id = store.objects_mut().id();
        let env = env.as_v8().clone();
        let ty = ty.into();
        let handle = host(&mut store, &ty, move |args| {
            let env =
                AsyncFunctionEnvMut(BackendAsyncFunctionEnvMut::V8(env::AsyncFunctionEnvMut {
                    store: StoreAsync::from_context(id),
                    func_env: env.clone(),
                }));
            Box::pin(func(env, args))
        });
        unsafe { Self::from_owned(handle, id) }
    }
    pub(crate) fn new_typed_async<F, Args, Rets>(store: &mut impl AsStoreMut, func: F) -> Self
    where
        Args: WasmTypeList + 'static,
        Rets: WasmTypeList + 'static,
        F: AsyncHostFunction<(), Args, Rets, WithoutEnv> + 'static,
    {
        let env = FunctionEnv::new(store, ());
        Self::new_typed_host(store, &env, move |_, args| {
            func.call_async(AsyncFunctionEnv::new(), args)
        })
    }

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
        Self::new_typed_host(store, env, move |env, args| {
            func.call_async(AsyncFunctionEnv::with_env(env), args)
        })
    }

    fn new_typed_host<T, Args, Rets>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        func: impl Fn(AsyncFunctionEnvMut<T>, Args) -> LocalFuture<Result<Rets, RuntimeError>> + 'static,
    ) -> Self
    where
        T: 'static,
        Args: WasmTypeList + 'static,
        Rets: WasmTypeList + 'static,
    {
        let id = store.objects_mut().id();
        let signature = FunctionType::new(Args::wasm_types(), Rets::wasm_types());
        Self::new_with_env_async(store, env, signature, move |env, values| {
            let args = {
                let mut current = unsafe { StoreContext::get_current(id) };
                let mut raw = Args::empty_array();
                for (slot, value) in raw.as_mut().iter_mut().zip(values) {
                    *slot = value.as_raw(&current.as_ref());
                }
                unsafe { Args::from_array(&mut current.as_mut(), raw) }
            };
            let async_store = Turn::current(id).and_then(|_| StoreAsync::from_context(id));
            let future = func(env, args);
            async move {
                let result = future.await?;
                let to_values = |store: &mut StoreMut| {
                    let mut raw = unsafe { result.into_array(store) };
                    raw.as_mut()
                        .iter()
                        .zip(Rets::wasm_types())
                        .map(|(raw, ty)| unsafe { Value::from_raw(store, *ty, *raw) })
                        .collect()
                };
                if let Some(store) = async_store {
                    Ok(to_values(&mut store.write_lock().await.as_store_mut()))
                } else {
                    // A synchronous invocation polls once before leaving its store context.
                    let mut current = unsafe { StoreContext::get_current(id) };
                    Ok(to_values(&mut current.as_mut()))
                }
            }
        })
    }
}
