use crate::exports::{ExportError, Exportable};
use crate::externals::Extern;
use crate::store::Store;
use crate::types::Val;
use crate::FunctionType;
use crate::NativeFunc;
use crate::RuntimeError;
use std::cmp::max;
use wasm_common::{HostFunction, WasmTypeList, WithEnv, WithoutEnv};
use wasmer_runtime::{
    wasmer_call_trampoline, Export, ExportFunction, VMCallerCheckedAnyfunc, VMContext,
    VMDynamicFunctionContext, VMFunctionBody, VMFunctionKind, VMTrampoline,
};

/// A function defined in the Wasm module
#[derive(Clone, PartialEq)]
pub struct WasmFunctionDefinition {
    // The trampoline to do the call
    pub(crate) trampoline: VMTrampoline,
}

/// The inner helper
#[derive(Clone, PartialEq)]
pub enum FunctionDefinition {
    /// A function defined in the Wasm side
    Wasm(WasmFunctionDefinition),
    /// A function defined in the Host side
    Host,
}

/// A WebAssembly `function`.
#[derive(Clone, PartialEq)]
pub struct Function {
    pub(crate) store: Store,
    pub(crate) definition: FunctionDefinition,
    // If the Function is owned by the Store, not the instance
    pub(crate) owned_by_store: bool,
    pub(crate) has_env: bool,
    pub(crate) exported: ExportFunction,
}

impl Function {
    /// Creates a new `Func` with the given parameters.
    ///
    /// * `store` - a global cache to store information in
    /// * `func` - the function.
    pub fn new<F, Args, Rets, Env>(store: &Store, func: F) -> Self
    where
        F: HostFunction<Args, Rets, WithoutEnv, Env>,
        Args: WasmTypeList,
        Rets: WasmTypeList,
        Env: Sized,
    {
        let func: wasm_common::Func<Args, Rets> = wasm_common::Func::new(func);
        let address = func.address() as *const VMFunctionBody;
        let vmctx = std::ptr::null_mut() as *mut _ as *mut VMContext;
        let signature = func.ty();
        Self {
            store: store.clone(),
            owned_by_store: true,
            definition: FunctionDefinition::Host,
            has_env: false,
            exported: ExportFunction {
                address,
                vmctx,
                signature,
                kind: VMFunctionKind::Static,
            },
        }
    }

    #[allow(clippy::cast_ptr_alignment)]
    pub fn new_dynamic<F>(store: &Store, ty: &FunctionType, func: F) -> Self
    where
        F: Fn(&[Val]) -> Result<Vec<Val>, RuntimeError> + 'static,
    {
        let dynamic_ctx = VMDynamicFunctionContext::from_context(VMDynamicFunctionWithoutEnv {
            func: Box::new(func),
            function_type: ty.clone(),
        });
        // We don't yet have the address with the Wasm ABI signature.
        // The engine linker will replace the address with one pointing to a
        // generated dynamic trampoline.
        let address = std::ptr::null() as *const VMFunctionBody;
        let vmctx = Box::into_raw(Box::new(dynamic_ctx)) as *mut VMContext;
        Self {
            store: store.clone(),
            owned_by_store: true,
            definition: FunctionDefinition::Host,
            has_env: false,
            exported: ExportFunction {
                address,
                kind: VMFunctionKind::Dynamic,
                vmctx,
                signature: ty.clone(),
            },
        }
    }

    #[allow(clippy::cast_ptr_alignment)]
    pub fn new_dynamic_env<F, Env>(store: &Store, ty: &FunctionType, env: &mut Env, func: F) -> Self
    where
        F: Fn(&mut Env, &[Val]) -> Result<Vec<Val>, RuntimeError> + 'static,
        Env: Sized,
    {
        let dynamic_ctx = VMDynamicFunctionContext::from_context(VMDynamicFunctionWithEnv {
            env,
            func: Box::new(func),
            function_type: ty.clone(),
        });
        // We don't yet have the address with the Wasm ABI signature.
        // The engine linker will replace the address with one pointing to a
        // generated dynamic trampoline.
        let address = std::ptr::null() as *const VMFunctionBody;
        let vmctx = Box::into_raw(Box::new(dynamic_ctx)) as *mut VMContext;
        Self {
            store: store.clone(),
            owned_by_store: true,
            definition: FunctionDefinition::Host,
            has_env: true,
            exported: ExportFunction {
                address,
                kind: VMFunctionKind::Dynamic,
                vmctx,
                signature: ty.clone(),
            },
        }
    }

    /// Creates a new `Func` with the given parameters.
    ///
    /// * `store` - a global cache to store information in.
    /// * `env` - the function environment.
    /// * `func` - the function.
    pub fn new_env<F, Args, Rets, Env>(store: &Store, env: &mut Env, func: F) -> Self
    where
        F: HostFunction<Args, Rets, WithEnv, Env>,
        Args: WasmTypeList,
        Rets: WasmTypeList,
        Env: Sized,
    {
        let func: wasm_common::Func<Args, Rets> = wasm_common::Func::new(func);
        let address = func.address() as *const VMFunctionBody;
        // TODO: We need to refactor the Function context.
        // Right now is structured as it's always a `VMContext`. However, only
        // Wasm-defined functions have a `VMContext`.
        // In the case of Host-defined functions `VMContext` is whatever environment
        // the user want to attach to the function.
        let vmctx = env as *mut _ as *mut VMContext;
        let signature = func.ty();
        Self {
            store: store.clone(),
            owned_by_store: true,
            definition: FunctionDefinition::Host,
            has_env: true,
            exported: ExportFunction {
                address,
                kind: VMFunctionKind::Static,
                vmctx,
                signature,
            },
        }
    }

    /// Returns the underlying type of this function.
    pub fn ty(&self) -> &FunctionType {
        &self.exported.signature
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    fn call_wasm(
        &self,
        func: &WasmFunctionDefinition,
        params: &[Val],
        results: &mut [Val],
    ) -> Result<(), RuntimeError> {
        let format_types_for_error_message = |items: &[Val]| {
            items
                .iter()
                .map(|param| param.ty().to_string())
                .collect::<Vec<String>>()
                .join(", ")
        };
        let signature = self.ty();
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

        let mut values_vec = vec![0; max(params.len(), results.len())];

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
            unsafe {
                arg.write_value_to(slot);
            }
        }

        // Call the trampoline.
        if let Err(error) = unsafe {
            wasmer_call_trampoline(
                self.exported.vmctx,
                func.trampoline,
                self.exported.address,
                values_vec.as_mut_ptr() as *mut u8,
            )
        } {
            return Err(RuntimeError::from_trap(error));
        }

        // Load the return values out of `values_vec`.
        for (index, &value_type) in signature.results().iter().enumerate() {
            unsafe {
                let ptr = values_vec.as_ptr().add(index);
                results[index] = Val::read_value_from(ptr, value_type);
            }
        }

        Ok(())
    }

    /// Returns the number of parameters that this function takes.
    pub fn param_arity(&self) -> usize {
        self.ty().params().len()
    }

    /// Returns the number of results this function produces.
    pub fn result_arity(&self) -> usize {
        self.ty().results().len()
    }

    /// Call the [`Function`] function.
    ///
    /// Depending on where the Function is defined, it will call it.
    /// 1. If the function is defined inside a WebAssembly, it will call the trampoline
    ///    for the function signature.
    /// 2. If the function is defined in the host (in a native way), it will
    ///    call the trampoline.
    pub fn call(&self, params: &[Val]) -> Result<Box<[Val]>, RuntimeError> {
        let mut results = vec![Val::null(); self.result_arity()];

        match &self.definition {
            FunctionDefinition::Wasm(wasm) => {
                self.call_wasm(&wasm, params, &mut results)?;
            }
            _ => unimplemented!("The function definition isn't supported for the moment"),
        }

        Ok(results.into_boxed_slice())
    }

    pub(crate) fn from_export(store: &Store, wasmer_export: ExportFunction) -> Self {
        let vmsignature = store.engine().register_signature(&wasmer_export.signature);
        let trampoline = store
            .engine()
            .function_call_trampoline(vmsignature)
            .expect("Can't get call trampoline for the function");
        Self {
            store: store.clone(),
            owned_by_store: false,
            has_env: true,
            definition: FunctionDefinition::Wasm(WasmFunctionDefinition { trampoline }),
            exported: wasmer_export,
        }
    }

    pub(crate) fn checked_anyfunc(&self) -> VMCallerCheckedAnyfunc {
        let vmsignature = self
            .store
            .engine()
            .register_signature(&self.exported.signature);
        VMCallerCheckedAnyfunc {
            func_ptr: self.exported.address,
            type_index: vmsignature,
            vmctx: self.exported.vmctx,
        }
    }

    pub fn native<'a, Args, Rets>(&self) -> Option<NativeFunc<'a, Args, Rets>>
    where
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        // type check
        if self.exported.signature.params() != Args::wasm_types() {
            // todo: error param types don't match
            return None;
        }
        if self.exported.signature.results() != Rets::wasm_types() {
            // todo: error result types don't match
            return None;
        }

        Some(NativeFunc::new(
            self.store.clone(),
            self.exported.address,
            self.exported.vmctx,
            self.exported.kind,
            self.definition.clone(),
            self.has_env,
        ))
    }
}

impl<'a> Exportable<'a> for Function {
    fn to_export(&self) -> Export {
        self.exported.clone().into()
    }
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Function(func) => Ok(func),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}

impl std::fmt::Debug for Function {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

/// This trait is one that all dynamic functions must fulfill.
pub(crate) trait VMDynamicFunction {
    fn call(&self, args: &[Val]) -> Result<Vec<Val>, RuntimeError>;
    fn function_type(&self) -> &FunctionType;
}

pub(crate) struct VMDynamicFunctionWithoutEnv {
    #[allow(clippy::type_complexity)]
    func: Box<dyn Fn(&[Val]) -> Result<Vec<Val>, RuntimeError> + 'static>,
    function_type: FunctionType,
}

impl VMDynamicFunction for VMDynamicFunctionWithoutEnv {
    fn call(&self, args: &[Val]) -> Result<Vec<Val>, RuntimeError> {
        (*self.func)(&args)
    }
    fn function_type(&self) -> &FunctionType {
        &self.function_type
    }
}

pub(crate) struct VMDynamicFunctionWithEnv<Env>
where
    Env: Sized,
{
    #[allow(clippy::type_complexity)]
    func: Box<dyn Fn(&mut Env, &[Val]) -> Result<Vec<Val>, RuntimeError> + 'static>,
    env: *mut Env,
    function_type: FunctionType,
}

impl<Env> VMDynamicFunction for VMDynamicFunctionWithEnv<Env>
where
    Env: Sized,
{
    fn call(&self, args: &[Val]) -> Result<Vec<Val>, RuntimeError> {
        unsafe { (*self.func)(&mut *self.env, &args) }
    }
    fn function_type(&self) -> &FunctionType {
        &self.function_type
    }
}

trait VMDynamicFunctionCall<T: VMDynamicFunction> {
    fn from_context(ctx: T) -> Self;
    fn address_ptr() -> *const VMFunctionBody;
    unsafe fn func_wrapper(&self, values_vec: *mut i128);
}

impl<T: VMDynamicFunction> VMDynamicFunctionCall<T> for VMDynamicFunctionContext<T> {
    fn from_context(ctx: T) -> Self {
        Self {
            address: Self::address_ptr(),
            ctx,
        }
    }

    fn address_ptr() -> *const VMFunctionBody {
        Self::func_wrapper as *const () as *const VMFunctionBody
    }

    // This function wraps our func, to make it compatible with the
    // reverse trampoline signature
    unsafe fn func_wrapper(
        // Note: we use the trick that the first param to this function is the `VMDynamicFunctionContext`
        // itself, so rather than doing `dynamic_ctx: &VMDynamicFunctionContext<T>`, we simplify it a bit
        &self,
        values_vec: *mut i128,
    ) {
        use std::panic::{self, AssertUnwindSafe};
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            let func_ty = self.ctx.function_type();
            let mut args = Vec::with_capacity(func_ty.params().len());
            for (i, ty) in func_ty.params().iter().enumerate() {
                args.push(Val::read_value_from(values_vec.add(i), *ty));
            }
            let returns = self.ctx.call(&args)?;

            // We need to dynamically check that the returns
            // match the expected types, as well as expected length.
            let return_types = returns.iter().map(|ret| ret.ty()).collect::<Vec<_>>();
            if return_types != func_ty.results() {
                return Err(RuntimeError::new(format!(
                    "Dynamic function returned wrong signature. Expected {:?} but got {:?}",
                    func_ty.results(),
                    return_types
                )));
            }
            for (i, ret) in returns.iter().enumerate() {
                ret.write_value_to(values_vec.add(i));
            }
            Ok(())
        }));

        match result {
            Ok(Ok(())) => {}
            Ok(Err(trap)) => wasmer_runtime::raise_user_trap(Box::new(trap)),
            Err(panic) => wasmer_runtime::resume_panic(panic),
        }
    }
}
