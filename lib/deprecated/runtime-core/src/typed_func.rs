use crate::{
    error::RuntimeError,
    new,
    types::FuncDescriptor,
    types::{Type, Value},
    vm,
};
use std::ptr;

pub struct Func {
    new_function: new::wasmer::Function,
}

impl Func {
    pub fn new<F, Args, Rets>(func: F) -> Self
    where
        F: new::wasm_common::HostFunction<Args, Rets, new::wasm_common::WithEnv, vm::Ctx>,
        Args: new::wasm_common::WasmTypeList,
        Rets: new::wasm_common::WasmTypeList,
    {
        // Create a fake `vm::Ctx`, that is going to be overwritten by `Instance::new`.
        let ctx: &mut vm::Ctx = unsafe { &mut *ptr::null_mut() };

        let store = Default::default();

        Self {
            new_function: new::wasmer::Function::new_env(&store, ctx, func),
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
