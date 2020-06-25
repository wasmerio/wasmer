use crate::{
    error::{ExportError, RuntimeError},
    new,
    types::{FuncDescriptor, Type, Value},
    vm,
};
use std::marker::PhantomData;

#[derive(Clone)]
pub struct Func<Args, Rets>
where
    Args: new::wasmer::WasmTypeList,
    Rets: new::wasmer::WasmTypeList,
{
    new_function: new::wasmer::Function,
    _phantom: PhantomData<(Args, Rets)>,
}

impl<Args, Rets> Func<Args, Rets>
where
    Args: new::wasmer::WasmTypeList,
    Rets: new::wasmer::WasmTypeList,
{
    pub fn new<F>(func: F) -> Self
    where
        F: new::wasmer::HostFunction<Args, Rets, new::wasmer::WithEnv, vm::Ctx>,
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
            _phantom: PhantomData,
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

    pub fn dyn_call(&self, params: &[Value]) -> Result<Box<[Value]>, RuntimeError> {
        self.new_function.call(params)
    }
}

macro_rules! func_call {
    ( $( $x:ident ),* ) => {
        #[allow(unused_parens)]
        impl< $( $x, )* Rets > Func<( $( $x ),* ), Rets>
        where
            $( $x: new::wasmer::WasmExternType, )*
            Rets: new::wasmer::WasmTypeList
        {
            #[allow(non_snake_case, clippy::too_many_arguments)]
            pub fn call(&self, $( $x: $x, )* ) -> Result<Rets, RuntimeError> {
                self.new_function.native::<( $( $x ),* ), Rets>().unwrap().call( $( $x ),* )
            }
        }
    }
}

//func_call!();
//func_call!(A1);
//func_call!(A1, A2);

impl<Args, Rets> From<Func<Args, Rets>> for new::wasmer::Extern
where
    Args: new::wasmer::WasmTypeList,
    Rets: new::wasmer::WasmTypeList,
{
    fn from(func: Func<Args, Rets>) -> Self {
        new::wasmer::Extern::Function(func.new_function)
    }
}

impl<Args, Rets> From<&new::wasmer::Function> for Func<Args, Rets>
where
    Args: new::wasmer::WasmTypeList,
    Rets: new::wasmer::WasmTypeList,
{
    fn from(new_function: &new::wasmer::Function) -> Self {
        Self {
            new_function: new_function.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<'a, Args, Rets> new::wasmer::Exportable<'a> for Func<Args, Rets>
where
    Args: new::wasmer::WasmTypeList,
    Rets: new::wasmer::WasmTypeList,
{
    fn to_export(&self) -> new::wasmer_runtime::Export {
        self.new_function.to_export()
    }

    fn get_self_from_extern(r#extern: &'a new::wasmer::Extern) -> Result<&'a Self, ExportError> {
        match r#extern {
            new::wasmer::Extern::Function(func) => Ok(
                // It's not ideal to call `Box::leak` here, but it would introduce too much changes in the `new::wasmer` API to support `Cow` or similar.
                Box::leak(Box::<Func<Args, Rets>>::new(func.into())),
            ),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
