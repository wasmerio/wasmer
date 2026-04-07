//! Data types, functions and traits for `wasmi`'s `Function` implementation.

#![allow(non_snake_case)]
#![allow(missing_docs)]

use std::panic::{self, AssertUnwindSafe};

use ::wasmi;
use crate::{
    AsStoreMut, AsStoreRef, BackendFunction, FunctionEnv, FunctionEnvMut, HostFunction,
    RuntimeError, StoreMut, Value, WasmTypeList, WithEnv, WithoutEnv,
    vm::{VMExtern, VMExternFunction},
    wasmi::{
        utils::convert::{IntoCApiType, IntoCApiValue, IntoWasmerType, IntoWasmerValue},
        vm::{handle_bits, VMFuncRef, VMFunction, VMFunctionCallback},
    },
};
use wasmer_types::{FunctionType, RawValue};

pub(crate) mod env;
pub(crate) mod typed;

pub use typed::*;

#[derive(Clone)]
/// A WebAssembly `function` in `wasmi`.
pub struct Function {
    pub(crate) handle: VMFunction,
}

unsafe impl Send for Function {}
unsafe impl Sync for Function {}

impl PartialEq for Function {
    fn eq(&self, other: &Self) -> bool {
        handle_bits(self.handle) == handle_bits(other.handle)
    }
}

impl Eq for Function {}

impl From<VMFunction> for Function {
    fn from(handle: VMFunction) -> Self {
        Self { handle }
    }
}

fn wasmi_func_type(ty: &FunctionType) -> wasmi::FuncType {
    wasmi::FuncType::new(
        ty.params().iter().copied().map(|param| param.into_ct()),
        ty.results().iter().copied().map(|result| result.into_ct()),
    )
}

fn wasmi_error_from_runtime_error(err: RuntimeError) -> wasmi::Error {
    crate::backend::wasmi::error::Trap::user(Box::new(err)).into_wasmi_error()
}

fn run_call_with_on_called<T, F>(
    store: &mut impl AsStoreMut,
    mut f: F,
) -> Result<T, RuntimeError>
where
    F: FnMut(&mut crate::backend::wasmi::store::Store) -> Result<T, wasmi::Error>,
{
    loop {
        let result = {
            let store_mut = store.as_store_mut();
            f(store_mut.inner.store.as_wasmi_mut())
        };

        let store_mut = store.as_store_mut();
        if let Some(callback) = store_mut.inner.on_called.take() {
            match callback(store_mut) {
                Ok(wasmer_types::OnCalledAction::InvokeAgain) => continue,
                Ok(wasmer_types::OnCalledAction::Finish) => {
                    return result.map_err(crate::backend::wasmi::error::Trap::from_wasmi_error)
                }
                Ok(wasmer_types::OnCalledAction::Trap(trap)) => return Err(RuntimeError::user(trap)),
                Err(trap) => return Err(RuntimeError::user(trap)),
            }
        }

        return result.map_err(crate::backend::wasmi::error::Trap::from_wasmi_error);
    }
}

impl Function {
    /// To `VMExtern`.
    pub fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Wasmi(crate::backend::wasmi::vm::VMExtern::Function(self.handle))
    }

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
        let function_type: FunctionType = ty.into();
        let wasmi_ty = wasmi_func_type(&function_type);
        let mut store = store.as_store_mut();
        let raw_store = store.as_raw() as usize;
        let env_handle = env.as_wasmi().handle.clone();

        let handle = wasmi::Func::new(
            &mut store.inner.store.as_wasmi_mut().inner,
            wasmi_ty,
            move |_caller, inputs, outputs| {
                let mut store = unsafe { StoreMut::from_raw(raw_store as *mut _) };
                let fn_env = env::FunctionEnv::from_handle(env_handle.clone()).into_mut(&mut store);
                let args: Vec<Value> = inputs.iter().cloned().map(IntoWasmerValue::into_wv).collect();

                let result = panic::catch_unwind(AssertUnwindSafe(|| func(fn_env.into(), &args)));
                let values = match result {
                    Ok(Ok(values)) => values,
                    Ok(Err(err)) => return Err(wasmi_error_from_runtime_error(err)),
                    Err(_) => return Err(wasmi::Error::new("host function panicked")),
                };

                if values.len() != outputs.len() {
                    return Err(wasmi::Error::new(
                        "host function returned unexpected number of results",
                    ));
                }

                for (dst, value) in outputs.iter_mut().zip(values.into_iter()) {
                    *dst = value.into_cv();
                }
                Ok(())
            },
        );

        Self { handle }
    }

    pub fn new_typed<F, Args, Rets>(store: &mut impl AsStoreMut, func: F) -> Self
    where
        F: HostFunction<(), Args, Rets, WithoutEnv> + 'static + Send + Sync,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        let env = FunctionEnv::new(store, ());
        Self::new_with_env(
            store,
            &env,
            FunctionType::new(Args::wasm_types(), Rets::wasm_types()),
            move |env, values| func.call_wasm(env, values),
        )
    }

    pub fn new_typed_with_env<T, F, Args, Rets>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        func: F,
    ) -> Self
    where
        F: HostFunction<T, Args, Rets, WithEnv>
            + 'static
            + Send
            + Sync,
        Args: WasmTypeList,
        Rets: WasmTypeList,
        T: Send + 'static,
    {
        Self::new_with_env(
            store,
            env,
            FunctionType::new(Args::wasm_types(), Rets::wasm_types()),
            move |env, values| func.call_wasm(env, values),
        )
    }

    pub fn ty(&self, store: &impl AsStoreRef) -> FunctionType {
        let ty = self.handle.ty(&store.as_store_ref().inner.store.as_wasmi().inner);
        FunctionType::new(
            ty.params()
                .iter()
                .copied()
                .map(IntoWasmerType::into_wt)
                .collect::<Vec<_>>(),
            ty.results()
                .iter()
                .copied()
                .map(IntoWasmerType::into_wt)
                .collect::<Vec<_>>(),
        )
    }

    pub fn call_raw(
        &self,
        _store: &mut impl AsStoreMut,
        _params: Vec<RawValue>,
    ) -> Result<Box<[Value]>, RuntimeError> {
        unimplemented!();
    }

    pub fn call(
        &self,
        store: &mut impl AsStoreMut,
        params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        let args: Vec<wasmi::Val> = params.iter().cloned().map(IntoCApiValue::into_cv).collect();
        let result_types = self.ty(store).results().to_vec();
        let mut results: Vec<wasmi::Val> = result_types
            .iter()
            .copied()
            .map(|ty| wasmi::Val::default(ty.into_ct()))
            .collect();

        run_call_with_on_called(store, |store_inner| {
            self.handle.call(&mut store_inner.inner, &args, &mut results)
        })?;

        Ok(results
            .into_iter()
            .map(IntoWasmerValue::into_wv)
            .collect::<Vec<_>>()
            .into_boxed_slice())
    }

    pub(crate) fn from_vm_extern(
        _store: &mut impl AsStoreMut,
        internal: VMExternFunction,
    ) -> Self {
        let crate::vm::VMExternFunction::Wasmi(handle) = internal else {
            panic!("Not a `wasmi` function extern")
        };
        Self { handle }
    }

    pub(crate) fn vm_funcref(&self, _store: &impl AsStoreRef) -> VMFuncRef {
        unimplemented!()
    }

    pub(crate) unsafe fn from_vm_funcref(
        _store: &mut impl AsStoreMut,
        _funcref: VMFuncRef,
    ) -> Self {
        unimplemented!()
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }
}

macro_rules! stub_host_callback_generators {
    ($($name:ident),* $(,)?) => {
        $(
            pub(crate) fn $name<_T>(_this: &_T) -> VMFunctionCallback {
                std::ptr::null_mut()
            }
        )*
    };
}

stub_host_callback_generators!(
    gen_fn_callback_s0,
    gen_fn_callback_s0_no_env,
    gen_fn_callback_s1,
    gen_fn_callback_s1_no_env,
    gen_fn_callback_s2,
    gen_fn_callback_s2_no_env,
    gen_fn_callback_s3,
    gen_fn_callback_s3_no_env,
    gen_fn_callback_s4,
    gen_fn_callback_s4_no_env,
    gen_fn_callback_s5,
    gen_fn_callback_s5_no_env,
    gen_fn_callback_s6,
    gen_fn_callback_s6_no_env,
    gen_fn_callback_s7,
    gen_fn_callback_s7_no_env,
    gen_fn_callback_s8,
    gen_fn_callback_s8_no_env,
    gen_fn_callback_s9,
    gen_fn_callback_s9_no_env,
    gen_fn_callback_s10,
    gen_fn_callback_s10_no_env,
    gen_fn_callback_s11,
    gen_fn_callback_s11_no_env,
    gen_fn_callback_s12,
    gen_fn_callback_s12_no_env,
    gen_fn_callback_s13,
    gen_fn_callback_s13_no_env,
    gen_fn_callback_s14,
    gen_fn_callback_s14_no_env,
    gen_fn_callback_s15,
    gen_fn_callback_s15_no_env,
    gen_fn_callback_s16,
    gen_fn_callback_s16_no_env,
    gen_fn_callback_s17,
    gen_fn_callback_s17_no_env,
    gen_fn_callback_s18,
    gen_fn_callback_s18_no_env,
    gen_fn_callback_s19,
    gen_fn_callback_s19_no_env,
    gen_fn_callback_s20,
    gen_fn_callback_s20_no_env,
    gen_fn_callback_s21,
    gen_fn_callback_s21_no_env,
    gen_fn_callback_s22,
    gen_fn_callback_s22_no_env,
    gen_fn_callback_s23,
    gen_fn_callback_s23_no_env,
    gen_fn_callback_s24,
    gen_fn_callback_s24_no_env,
    gen_fn_callback_s25,
    gen_fn_callback_s25_no_env,
    gen_fn_callback_s26,
    gen_fn_callback_s26_no_env,
);

impl std::fmt::Debug for Function {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.debug_struct("Function").finish()
    }
}

impl crate::Function {
    pub fn into_wasmi(self) -> crate::backend::wasmi::function::Function {
        match self.0 {
            BackendFunction::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` function!"),
        }
    }

    pub fn as_wasmi(&self) -> &crate::backend::wasmi::function::Function {
        match &self.0 {
            BackendFunction::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` function!"),
        }
    }

    pub fn as_wasmi_mut(&mut self) -> &mut crate::backend::wasmi::function::Function {
        match &mut self.0 {
            BackendFunction::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` function!"),
        }
    }
}
