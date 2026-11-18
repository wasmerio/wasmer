//! Drive a JSPI continuation on the store's thread. The adapters pass guest values
//! through Wasm and use a zero-argument JS function for suspension.
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
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::c_void,
    future::Future,
    panic::{AssertUnwindSafe, catch_unwind},
    pin::Pin,
    ptr,
    rc::Rc,
};
use wasm_encoder::{
    BlockType, CodeSection, EntityType, ExportKind, ExportSection, FunctionSection, ImportSection,
    Instruction, Module, TypeSection, ValType,
};
use wasmer_types::{FunctionType, StoreId, TrapCode, Type};

type Values = Result<Vec<Value>, RuntimeError>;
type LocalFuture<T> = Pin<Box<dyn Future<Output = T>>>;
type HostFuture = LocalFuture<Values>;
type HostCallback = dyn Fn(&[Value]) -> Values;

#[derive(Default)]
pub(crate) struct Adapters {
    exports: HashMap<*mut wasm_func_t, *mut wasm_func_t>,
    suspender: Option<*mut wasm_func_t>,
    handles: Vec<*mut wasm_func_t>,
    // V8 holds pointers to these callbacks until the isolate is disposed.
    #[allow(clippy::vec_box)]
    callbacks: Vec<Box<Callback>>,
}

impl Drop for Adapters {
    fn drop(&mut self) {
        for function in self.handles.drain(..) {
            unsafe { wasm_func_delete(function) }
        }
    }
}

struct Callback {
    store: StoreId,
    results: Box<[Type]>,
    function: Box<HostCallback>,
}

struct Turn {
    store: StoreId,
    values: Cell<Vec<Value>>,
    host_error: Cell<Option<RuntimeError>>,
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

struct Continuation(*mut wasm_jspi_call_t);
impl Drop for Continuation {
    fn drop(&mut self) {
        unsafe { wasm_jspi_call_delete(self.0) }
    }
}

fn valtype(ty: Type) -> ValType {
    match ty {
        Type::I32 => ValType::I32,
        Type::I64 => ValType::I64,
        Type::F32 => ValType::F32,
        Type::F64 => ValType::F64,
        Type::V128 => ValType::V128,
        Type::FuncRef => ValType::FUNCREF,
        Type::ExternRef => ValType::EXTERNREF,
        Type::ExceptionRef => ValType::EXNREF,
    }
}

// Each adapter calls three imports. For a host import they start the host
// future, optionally suspend, and read its results. For an export they load
// the arguments, call the guest, and save the results.
fn adapter(
    store: &mut StoreMut<'_>,
    signature: &FunctionType,
    imports: [(&FunctionType, *mut wasm_func_t); 3],
    suspending: bool,
) -> Result<*mut wasm_func_t, RuntimeError> {
    let mut types = TypeSection::new();
    let mut import_section = ImportSection::new();
    for (index, (ty, _)) in imports.iter().enumerate() {
        types.ty().function(
            ty.params().iter().copied().map(valtype),
            ty.results().iter().copied().map(valtype),
        );
        import_section.import(
            "",
            ["start", "call", "finish"][index],
            EntityType::Function(index as u32),
        );
    }
    types.ty().function(
        signature.params().iter().copied().map(valtype),
        signature.results().iter().copied().map(valtype),
    );
    let mut functions = FunctionSection::new();
    functions.function(3);
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, 3);
    let mut body = wasm_encoder::Function::new([]);
    for index in 0..signature.params().len() {
        body.instruction(&Instruction::LocalGet(index as u32));
    }
    body.instruction(&Instruction::Call(0));
    if suspending {
        body.instruction(&Instruction::If(BlockType::Empty));
    }
    body.instruction(&Instruction::Call(1));
    if suspending {
        body.instruction(&Instruction::End);
    }
    body.instruction(&Instruction::Call(2));
    body.instruction(&Instruction::End);
    let mut code = CodeSection::new();
    code.function(&body);
    let mut module = Module::new();
    module
        .section(&types)
        .section(&import_section)
        .section(&functions)
        .section(&exports)
        .section(&code);
    let mut bytes = module.finish();
    let backend = store.inner.store.as_v8_mut();
    unsafe {
        let binary = wasm_byte_vec_t {
            size: bytes.len(),
            data: bytes.as_mut_ptr().cast(),
        };
        let module = wasm_module_new(backend.inner, &binary);
        if module.is_null() {
            return Err(RuntimeError::new("unable to compile V8 async adapter"));
        }
        let mut items = imports.map(|(_, func)| wasm_func_as_extern(func));
        let imports = wasm_extern_vec_t {
            size: items.len(),
            data: items.as_mut_ptr(),
        };
        let mut trap = ptr::null_mut();
        let instance = wasm_instance_new(backend.inner, module, &imports, &mut trap);
        wasm_module_delete(module);
        if instance.is_null() {
            return Err(Trap::from(trap).into());
        }
        let mut exports = wasm_extern_vec_t::default();
        wasm_instance_exports(instance, &mut exports);
        let function = wasm_func_copy(wasm_extern_as_func(*exports.data));
        wasm_extern_vec_delete(&mut exports);
        wasm_instance_delete(instance);
        backend.jspi.handles.push(function);
        Ok(function)
    }
}

impl Callback {
    // The C API lends reference arguments until the callback returns. Keep
    // an owned function handle in the store if Rust retains it in a future.
    unsafe fn argument(&self, val: wasm_val_t) -> Result<Value, RuntimeError> {
        match val.kind as wasm_valkind_enum {
            bindings::wasm_valkind_enum_WASM_FUNCREF => {
                let reference = unsafe { val.of.ref_ };
                if reference.is_null() {
                    return Ok(Value::FuncRef(None));
                }
                let mut current = unsafe { StoreContext::get_current(self.store) };
                let handle = unsafe { wasm_func_copy(wasm_ref_as_func(reference)) };
                current
                    .as_mut()
                    .inner
                    .store
                    .as_v8_mut()
                    .jspi
                    .handles
                    .push(handle);
                Ok(Value::FuncRef(Some(crate::Function(
                    crate::BackendFunction::V8(Function {
                        handle,
                        store_id: Some(self.store),
                    }),
                ))))
            }
            bindings::wasm_valkind_enum_WASM_EXTERNREF if unsafe { val.of.ref_ }.is_null() => {
                Ok(Value::ExternRef(None))
            }
            bindings::wasm_valkind_enum_WASM_EXTERNREF => Err(RuntimeError::new(
                "V8 does not support non-null externref host values",
            )),
            _ => Ok(val.into_wv()),
        }
    }

    fn result(&self, value: Value) -> Result<wasm_val_t, RuntimeError> {
        match value {
            Value::ExternRef(None) => Ok(wasm_val_t {
                kind: wasm_valkind_enum_WASM_EXTERNREF as _,
                of: wasm_val_t__bindgen_ty_1 {
                    ref_: ptr::null_mut(),
                },
            }),
            Value::FuncRef(Some(func)) => {
                let current = unsafe { StoreContext::get_current(self.store) };
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

    unsafe fn invoke(
        &self,
        args: &wasm_val_vec_t,
        results: &mut wasm_val_vec_t,
    ) -> Result<(), RuntimeError> {
        let values = (0..args.size)
            .map(|i| unsafe { self.argument(*args.data.add(i)) })
            .collect::<Result<Vec<_>, _>>()?;
        let returned = (self.function)(&values)?;
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
            unsafe { results.data.add(i).write(self.result(value)?) };
        }
        Ok(())
    }
}

fn host(
    store: &mut StoreMut<'_>,
    ty: &FunctionType,
    function: impl Fn(&[Value]) -> Values + 'static,
) -> *mut wasm_func_t {
    unsafe extern "C" fn invoke(
        env: *mut c_void,
        args: *const wasm_val_vec_t,
        results: *mut wasm_val_vec_t,
    ) -> *mut wasm_trap_t {
        let callback = unsafe { &*env.cast::<Callback>() };
        // The store owns the callback box. The C API owns both vectors for
        // this invocation, and sizes the output to the callback signature.
        let result = catch_unwind(AssertUnwindSafe(|| unsafe {
            callback.invoke(&*args, &mut *results)
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

    unsafe fn types(values: &[Type]) -> wasm_valtype_vec_t {
        let values: Vec<_> = values
            .iter()
            .map(|ty| unsafe { wasm_valtype_new(ty.into_ct()) })
            .collect();
        let mut output = wasm_valtype_vec_t::default();
        unsafe { wasm_valtype_vec_new(&mut output, values.len(), values.as_ptr()) };
        output
    }
    let mut data = Box::new(Callback {
        store: store.objects_mut().id(),
        results: ty.results().into(),
        function: Box::new(function),
    });
    let backend = store.inner.store.as_v8_mut();
    unsafe {
        let ty = wasm_functype_new(&mut types(ty.params()), &mut types(ty.results()));
        let func = wasm_func_new_with_env(
            backend.inner,
            ty,
            Some(invoke),
            (&mut *data as *mut Callback).cast(),
            None,
        );
        wasm_functype_delete(ty);
        assert!(!func.is_null(), "failed to create V8 async callback");
        backend.jspi.callbacks.push(data);
        backend.jspi.handles.push(func);
        func
    }
}

fn export_adapter(
    store: &mut StoreMut<'_>,
    function: &Function,
    ty: &FunctionType,
) -> Result<*mut wasm_func_t, RuntimeError> {
    let id = store.objects_mut().id();
    if let Some(handle) = store.inner.store.as_v8().jspi.exports.get(&function.handle) {
        return Ok(*handle);
    }
    let args = FunctionType::new([], ty.params().to_vec());
    let results = FunctionType::new(ty.results().to_vec(), []);
    let load = host(store, &args, move |_| {
        Ok(Turn::current(id).unwrap().values.take())
    });
    let save = host(store, &results, move |values| {
        Turn::current(id).unwrap().values.set(values.to_vec());
        Ok(vec![])
    });
    let handle = adapter(
        store,
        &FunctionType::new([], []),
        [(&args, load), (ty, function.handle), (&results, save)],
        false,
    )?;
    store
        .inner
        .store
        .as_v8_mut()
        .jspi
        .exports
        .insert(function.handle, handle);
    Ok(handle)
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
            let adapter = export_adapter(&mut lock.as_store_mut(), &function, &ty)?;
            let turn = Rc::new(Turn {
                store: store.id,
                values: Cell::new(params),
                host_error: Cell::new(None),
                pending: RefCell::default(),
            });
            let mut continuation = Continuation(ptr::null_mut());
            loop {
                let context = StoreContext::install_async(lock.inner);
                let trap = turn.enter(|| unsafe {
                    if continuation.0.is_null() {
                        wasm_jspi_start(adapter, &mut continuation.0)
                    } else {
                        wasm_jspi_resume(&mut continuation.0)
                    }
                });
                drop(context);
                if !trap.is_null() {
                    return Err(Trap::from(trap).into());
                }
                if continuation.0.is_null() {
                    return Ok(turn.values.take().into_boxed_slice());
                }
                let future = turn
                    .pending
                    .take()
                    .ok_or_else(|| RuntimeError::new("missing suspended V8 host future"))?;
                match future.await {
                    Ok(values) => turn.values.set(values),
                    Err(error) => turn.host_error.set(Some(error)),
                }
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
        let start_ty = FunctionType::new(ty.params().to_vec(), [Type::I32]);
        let finish_ty = FunctionType::new([], ty.results().to_vec());
        let immediate = Rc::new(Cell::new(Vec::new()));
        let immediate_result = immediate.clone();
        let start = host(&mut store, &start_ty, move |args| {
            let env =
                AsyncFunctionEnvMut(BackendAsyncFunctionEnvMut::V8(env::AsyncFunctionEnvMut {
                    store: StoreAsync::from_context(id),
                    func_env: env.clone(),
                }));
            let future = func(env, args);
            let pending = if let Some(turn) = Turn::current(id) {
                turn.pending.replace(Some(Box::pin(future)));
                true
            } else {
                let ready = future.now_or_never().ok_or_else(|| {
                    let code = TrapCode::YieldOutsideAsyncContext;
                    RuntimeError::new_from_source(
                        BackendTrap::V8(Trap::user(Box::new(code))),
                        vec![],
                        Some(code),
                    )
                })??;
                immediate_result.set(ready);
                false
            };
            Ok(vec![Value::I32(i32::from(pending))])
        });
        let finish = host(&mut store, &finish_ty, move |_| {
            let Some(turn) = Turn::current(id) else {
                return Ok(immediate.take());
            };
            if let Some(error) = turn.host_error.take() {
                return Err(error);
            }
            Ok(turn.values.take())
        });
        let backend = store.inner.store.as_v8_mut();
        let suspend = *backend.jspi.suspender.get_or_insert_with(|| {
            let suspend = unsafe { wasm_jspi_suspender(backend.inner) };
            assert!(!suspend.is_null(), "failed to create V8 JSPI suspender");
            backend.jspi.handles.push(suspend);
            suspend
        });
        let handle = adapter(
            &mut store,
            &ty,
            [
                (&start_ty, start),
                (&FunctionType::new([], []), suspend),
                (&finish_ty, finish),
            ],
            true,
        )
        .expect("valid V8 async import adapter");
        Self {
            handle,
            store_id: Some(id),
        }
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
