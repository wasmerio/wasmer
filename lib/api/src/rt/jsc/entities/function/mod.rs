pub(crate) mod env;
pub(crate) mod typed;

use rusty_jsc::{callback_closure, JSContext, JSObject, JSObjectCallAsFunctionCallback, JSValue};
use std::marker::PhantomData;
use wasmer_types::{FunctionType, RawValue};

pub(crate) use env::*;
pub(crate) use typed::*;

use crate::{
    jsc::{
        utils::convert::{jsc_value_to_wasmer, AsJsc},
        vm::{VMFuncRef, VMFunction},
    },
    vm::VMExtern,
    AsStoreMut, AsStoreRef, FunctionEnv, FunctionEnvMut, HostFunction, HostFunctionKind,
    RuntimeError, RuntimeFunction, StoreMut, Value, WasmTypeList, WithEnv, WithoutEnv,
};

use super::engine::IntoJSC;

#[derive(Clone, PartialEq, Eq)]
pub struct Function {
    pub(crate) handle: VMFunction,
}

// Function can't be Send in js because it dosen't support `structuredClone`
// https://developer.mozilla.org/en-US/docs/Web/API/structuredClone
// unsafe impl Send for Function {}

impl From<VMFunction> for Function {
    fn from(handle: VMFunction) -> Self {
        Self { handle }
    }
}

impl Function {
    /// To `VMExtern`.
    pub fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Jsc(crate::rt::jsc::vm::VMExtern::Function(self.handle.clone()))
    }

    #[allow(clippy::cast_ptr_alignment)]
    pub fn new_with_env<FT, F, T: Send + 'static>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        ty: FT,
        func: F,
    ) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(FunctionEnvMut<'_, T>, &[Value]) -> Result<Vec<Value>, RuntimeError>
            + 'static
            + Send
            + Sync,
    {
        let store = store.as_store_mut();
        let context = store.jsc().context();
        let function_type = ty.into();

        let new_function_type = function_type.clone();
        let raw_env = env.clone();

        let callback = callback_closure!(&context, move |ctx: JSContext,
                                                         function: JSObject,
                                                         this: JSObject,
                                                         args: &[JSValue]|
              -> Result<JSValue, JSValue> {
            let global = ctx.get_global_object();
            let store_ptr = global
                .get_property(&ctx, "__store_ptr".to_string())
                .to_number(&ctx)
                .unwrap();

            let mut store = unsafe { StoreMut::from_raw(store_ptr as usize as *mut _) };

            let env: FunctionEnvMut<T> = raw_env.clone().into_mut(&mut store);

            let wasm_arguments = new_function_type
                .params()
                .iter()
                .enumerate()
                .map(|(i, param)| jsc_value_to_wasmer(&ctx, param, &args[i]))
                .collect::<Vec<_>>();
            let results = func(env, &wasm_arguments).map_err(|e| {
                let value = format!("{}", e);
                JSValue::string(&ctx, value)
            })?;
            match new_function_type.results().len() {
                0 => Ok(JSValue::undefined(&ctx)),
                1 => Ok(results[0].as_jsc_value(&mut store)),
                _ => Ok(JSObject::new_array(
                    &ctx,
                    &results
                        .into_iter()
                        .map(|result| result.as_jsc_value(&mut store))
                        .collect::<Vec<_>>(),
                )?
                .to_jsvalue()),
            }
        });

        let vm_function = VMFunction::new(callback, function_type);
        Self {
            handle: vm_function,
        }
    }

    /// Creates a new host `Function` from a native function.
    pub fn new_typed<F, Args, Rets>(store: &mut impl AsStoreMut, func: F) -> Self
    where
        F: HostFunction<(), Args, Rets, WithoutEnv> + 'static + Send + Sync,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        let store = store.as_store_mut();
        let function = WasmFunction::<Args, Rets>::new(func);
        let callback = function.callback(store.jsc().context());

        let ty = function.ty();
        let vm_function = VMFunction::new(callback, ty);
        Self {
            handle: vm_function,
        }
    }

    pub fn new_typed_with_env<T, F, Args, Rets>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        func: F,
    ) -> Self
    where
        F: HostFunction<T, Args, Rets, WithEnv>,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        let store = store.as_store_mut();
        let context = store.jsc().context();
        let function = WasmFunction::<Args, Rets>::new(func);
        let callback = function.callback(store.jsc().context());

        let bind = callback
            .get_property(&context, "bind".to_string())
            .to_object(&context)
            .unwrap();
        let callback_with_env = bind
            .call(
                &context,
                Some(&callback),
                &[
                    JSValue::undefined(&context),
                    JSValue::number(
                        &context,
                        env.as_jsc().handle.internal_handle().index() as f64,
                    ),
                ],
            )
            .unwrap()
            .to_object(&context)
            .unwrap();

        let ty = function.ty();
        let vm_function = VMFunction::new(callback_with_env, ty);
        Self {
            handle: vm_function,
        }
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> FunctionType {
        self.handle.ty.clone()
    }

    pub fn call_raw(
        &self,
        _store: &mut impl AsStoreMut,
        _params: Vec<RawValue>,
    ) -> Result<Box<[Value]>, RuntimeError> {
        // There is no optimal call_raw in JSC, so we just
        // simply rely the call
        // self.call(store, params)
        unimplemented!();
    }

    pub fn call(
        &self,
        store: &mut impl AsStoreMut,
        params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        let store_mut = store.as_store_mut();
        let engine = store_mut.engine();
        let context = engine.as_jsc().context();

        let mut global = context.get_global_object();
        let store_ptr = store_mut.as_raw() as usize;
        global.set_property(
            &context,
            "__store_ptr".to_string(),
            JSValue::number(&context, store_ptr as _),
        );

        let params_list = params
            .iter()
            .map(|v| v.as_jsc_value(&store_mut))
            .collect::<Vec<_>>();
        let result = {
            let mut r;
            // TODO: This loop is needed for asyncify. It will be refactored with https://github.com/wasmerio/wasmer/issues/3451
            loop {
                let store_mut = store.as_store_mut();
                let engine = store_mut.engine();
                let context = engine.as_jsc().context();
                r = self.handle.function.call(&context, None, &params_list);
                if let Some(callback) = store_mut.inner.on_called.take() {
                    match callback(store_mut) {
                        Ok(wasmer_types::OnCalledAction::InvokeAgain) => {
                            continue;
                        }
                        Ok(wasmer_types::OnCalledAction::Finish) => {
                            break;
                        }
                        Ok(wasmer_types::OnCalledAction::Trap(trap)) => {
                            return Err(RuntimeError::user(trap))
                        }
                        Err(trap) => return Err(RuntimeError::user(trap)),
                    }
                }
                break;
            }
            r?
        };
        let result_types = self.handle.ty.results();
        let store_mut = store.as_store_mut();
        let engine = store_mut.engine();
        let context = engine.as_jsc().context();
        match result_types.len() {
            0 => Ok(Box::new([])),
            1 => {
                let value = jsc_value_to_wasmer(&context, &result_types[0], &result);
                Ok(vec![value].into_boxed_slice())
            }
            n => {
                let result = result.to_object(&context).unwrap();
                Ok((0..n)
                    .map(|i| {
                        let js_val = result.get_property_at_index(&context, i as _).unwrap();
                        jsc_value_to_wasmer(&context, &result_types[i], &js_val)
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice())
            }
        }
    }

    pub(crate) fn from_vm_extern(
        _store: &mut impl AsStoreMut,
        internal: crate::vm::VMExternFunction,
    ) -> Self {
        Self {
            handle: internal.into_jsc(),
        }
    }

    pub(crate) fn vm_funcref(&self, _store: &impl AsStoreRef) -> VMFuncRef {
        unimplemented!();
    }

    pub(crate) unsafe fn from_vm_funcref(
        _store: &mut impl AsStoreMut,
        _funcref: VMFuncRef,
    ) -> Self {
        unimplemented!();
    }

    /// Checks whether this `Function` can be used with the given context.
    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }
}

impl std::fmt::Debug for Function {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.debug_struct("Function").finish()
    }
}

/// Represents a low-level Wasm static host function. See
/// `super::Function::new` and `super::Function::new_env` to learn
/// more.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct WasmFunction<Args = (), Rets = ()> {
    callback: JSObjectCallAsFunctionCallback,
    _phantom: PhantomData<(Args, Rets)>,
}

unsafe impl<Args, Rets> Send for WasmFunction<Args, Rets> {}

impl<Args, Rets> WasmFunction<Args, Rets>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    /// Creates a new `WasmFunction`.
    #[allow(dead_code)]
    pub fn new<F, T, Kind: HostFunctionKind>(function: F) -> Self
    where
        F: HostFunction<T, Args, Rets, Kind>,
        T: Sized,
    {
        Self {
            callback: function.jsc_function_callback(),
            _phantom: PhantomData,
        }
    }

    /// Get the function type of this `WasmFunction`.
    #[allow(dead_code)]
    pub fn ty(&self) -> FunctionType {
        FunctionType::new(Args::wasm_types(), Rets::wasm_types())
    }

    /// Get the address of this `WasmFunction`.
    #[allow(dead_code)]
    pub fn callback(&self, context: &JSContext) -> JSObject {
        JSObject::new_function_with_callback(context, "FunctionCallback".to_string(), self.callback)
    }
}

impl crate::Function {
    /// Consume [`self`] into [`crate::rt::jsc::function::Function`].
    pub fn into_jsc(self) -> crate::rt::jsc::function::Function {
        match self.0 {
            RuntimeFunction::Jsc(s) => s,
            _ => panic!("Not a `jsc` function!"),
        }
    }

    /// Convert a reference to [`self`] into a reference to [`crate::rt::jsc::function::Function`].
    pub fn as_jsc(&self) -> &crate::rt::jsc::function::Function {
        match self.0 {
            RuntimeFunction::Jsc(ref s) => s,
            _ => panic!("Not a `jsc` function!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::rt::jsc::function::Function`].
    pub fn as_jsc_mut(&mut self) -> &mut crate::rt::jsc::function::Function {
        match self.0 {
            RuntimeFunction::Jsc(ref mut s) => s,
            _ => panic!("Not a `jsc` function!"),
        }
    }
}
