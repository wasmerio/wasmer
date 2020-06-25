use crate::{
    error::RuntimeError,
    new,
    types::FuncDescriptor,
    types::{Type, Value},
    vm,
};

pub struct Func {
    new_function: new::wasmer::Function,
}

impl Func {
    pub fn new<F, Args, Rets>(func: F) -> Self
    where
        F: new::wasmer::HostFunction<Args, Rets, new::wasmer::WithEnv, vm::Ctx>,
        Args: new::wasmer::WasmTypeList,
        Rets: new::wasmer::WasmTypeList,
    {
        // Create an empty `vm::Ctx`, that is going to be overwritten by `Instance::new`.
        let ctx = vm::Ctx::new();

        // TODO: check this, is incorrect. We should have a global store as we have in the
        // wasmer C API.
        let store = Default::default();

        Self {
            new_function: new::wasmer::Function::new_env::<F, Args, Rets, vm::Ctx>(
                &store, ctx, func,
            ),
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
