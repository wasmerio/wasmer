use crate::{
    error::RuntimeError,
    new,
    types::FuncDescriptor,
    types::{Type, Value},
};

pub struct Func {
    new_function: new::wasmer::Function,
}

impl Func {
    pub fn new<F, Args, Rets, Env>(func: F) -> Self
    where
        F: new::wasm_common::HostFunction<Args, Rets, new::wasm_common::WithoutEnv, Env>,
        Args: new::wasm_common::WasmTypeList,
        Rets: new::wasm_common::WasmTypeList,
        Env: Sized,
    {
        let store = Default::default();

        Self {
            new_function: new::wasmer::Function::new::<F, Args, Rets, Env>(&store, func),
        }
    }

    pub fn new_env<F, Args, Rets, Env>(env: &mut Env, func: F) -> Self
    where
        F: new::wasm_common::HostFunction<Args, Rets, new::wasm_common::WithEnv, Env>,
        Args: new::wasm_common::WasmTypeList,
        Rets: new::wasm_common::WasmTypeList,
        Env: Sized,
    {
        let store = Default::default();

        Self {
            new_function: new::wasmer::Function::new_env::<F, Args, Rets, Env>(&store, env, func),
        }
    }

    pub fn new_dynamic<F>(ty: &FuncDescriptor, func: F) -> Self
    where
        F: Fn(&[Value]) -> Result<Vec<Value>, RuntimeError> + 'static,
    {
        let store = Default::default();

        Self {
            new_function: new::wasmer::Function::new_dynamic(&store, ty, func),
        }
    }

    pub fn new_dynamic_env<F, Env>(ty: &FuncDescriptor, env: &mut Env, func: F) -> Self
    where
        F: Fn(&mut Env, &[Value]) -> Result<Vec<Value>, RuntimeError> + 'static,
        Env: Sized,
    {
        let store = Default::default();

        Self {
            new_function: new::wasmer::Function::new_dynamic_env::<F, Env>(&store, ty, env, func),
        }
    }

    pub fn signature(&self) -> &FuncDescriptor {
        self.new_function.ty()
    }

    pub fn params(&self) -> &[Type] {
        self.signature().params()
    }

    pub fn returns(&self) -> &[Type] {
        self.signature().results()
    }

    pub fn call(&self, params: &[Value]) -> Result<Box<[Value]>, RuntimeError> {
        self.new_function.call(params)
    }
}

impl From<Func> for new::wasmer::Extern {
    fn from(func: Func) -> Self {
        new::wasmer::Extern::Function(func.new_function)
    }
}

impl From<&new::wasmer::Function> for Func {
    fn from(new_function: &new::wasmer::Function) -> Self {
        Self {
            new_function: new_function.clone(),
        }
    }
}
