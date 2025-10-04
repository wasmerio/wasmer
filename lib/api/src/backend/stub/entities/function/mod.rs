use crate::entities::store::{AsStoreMut, AsStoreRef};
use crate::vm::{VMExtern, VMFuncRef};
use crate::{RuntimeError, Value};
use wasmer_types::FunctionType;

pub mod env;
pub use env::{FunctionEnv, FunctionEnvMut};

/// Minimal function placeholder for the stub backend.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Function {
    ty: FunctionType,
}

impl Function {
    pub fn new_with_env<FT, F, T: Send + 'static>(
        _store: &mut impl AsStoreMut,
        _env: &FunctionEnv<T>,
        ty: FT,
        _func: F,
    ) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(FunctionEnvMut<'_, T>, &[Value]) -> Result<Vec<Value>, RuntimeError>
            + 'static
            + Send
            + Sync,
    {
        Self { ty: ty.into() }
    }

    pub fn new_typed<F, Args, Rets>(
        _store: &mut impl AsStoreMut,
        _func: F,
    ) -> Self
    where
        F: crate::HostFunction<(), Args, Rets, crate::WithoutEnv> + 'static + Send + Sync,
        Args: crate::WasmTypeList,
        Rets: crate::WasmTypeList,
    {
        Self {
            ty: FunctionType::new([], []),
        }
    }

    pub fn new_typed_with_env<T: Send + 'static, F, Args, Rets>(
        _store: &mut impl AsStoreMut,
        _env: &FunctionEnv<T>,
        _func: F,
    ) -> Self
    where
        F: crate::HostFunction<T, Args, Rets, crate::WithEnv> + 'static + Send + Sync,
        Args: crate::WasmTypeList,
        Rets: crate::WasmTypeList,
    {
        Self {
            ty: FunctionType::new([], []),
        }
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> FunctionType {
        self.ty.clone()
    }

    pub fn call(
        &self,
        _store: &mut impl AsStoreMut,
        _params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        Err(RuntimeError::new(
            "The stub backend cannot execute WebAssembly functions",
        ))
    }

    pub fn call_raw(
        &self,
        _store: &mut impl AsStoreMut,
        _params: Vec<wasmer_types::RawValue>,
    ) -> Result<Box<[Value]>, RuntimeError> {
        Err(RuntimeError::new(
            "The stub backend cannot execute WebAssembly functions",
        ))
    }

    pub fn to_vm_extern(&self) -> VMExtern {
        VMExtern::stub()
    }

    pub fn vm_funcref(&self, _store: &impl AsStoreRef) -> VMFuncRef {
        VMFuncRef::stub()
    }

    pub unsafe fn from_vm_funcref(
        _store: &mut impl AsStoreMut,
        _funcref: VMFuncRef,
    ) -> Self {
        Self::default()
    }

    pub unsafe fn from_vm_extern(
        _store: &mut impl AsStoreMut,
        _extern_: VMExtern,
    ) -> Self {
        Self::default()
    }
}
