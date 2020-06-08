// Native Funcs
// use wasmer_runtime::ExportFunction;
use std::marker::PhantomData;

use crate::exports::{ExportError, Exportable};
use crate::externals::function::{FunctionDefinition, WasmFunctionDefinition};
use crate::{Extern, Function, FunctionType, Store};
use wasm_common::{NativeWasmType, WasmExternType, WasmTypeList};
use wasmer_runtime::{
    wasmer_call_trampoline, Export, ExportFunction, VMContext, VMFunctionBody, VMFunctionKind,
};

#[derive(Clone)]
pub struct UnprovidedArgs;
#[derive(Clone)]
pub struct UnprovidedRets;

pub struct NativeFunc<'a, Args = UnprovidedArgs, Rets = UnprovidedRets> {
    definition: FunctionDefinition,
    store: Store,
    address: *const VMFunctionBody,
    vmctx: *mut VMContext,
    arg_kind: VMFunctionKind,
    // exported: ExportFunction,
    _phantom: PhantomData<(&'a (), Args, Rets)>,
}

unsafe impl<'a, Args, Rets> Send for NativeFunc<'a, Args, Rets> {}

impl<'a, Args, Rets> NativeFunc<'a, Args, Rets> {
    pub(crate) fn new(
        store: Store,
        address: *const VMFunctionBody,
        vmctx: *mut VMContext,
        arg_kind: VMFunctionKind,
        definition: FunctionDefinition,
    ) -> Self {
        Self {
            definition,
            store,
            address,
            vmctx,
            arg_kind,
            _phantom: PhantomData,
        }
    }
}

impl<'a, Args, Rets> Exportable<'a> for NativeFunc<'a, Args, Rets>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    fn to_export(&self) -> Export {
        let ef: ExportFunction = self.into();
        ef.into()
    }

    // Cannot be implemented because of the return type `&Self` TODO:
    fn get_self_from_extern(extern_: &'a Extern) -> Result<Self, ExportError> {
        match extern_ {
            // TODO: review error return type in failure of `f.native()`
            Extern::Function(f) => f
                .clone()
                .native()
                .ok_or_else(|| ExportError::IncompatibleType),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}

impl<'a, Args, Rets> From<&NativeFunc<'a, Args, Rets>> for ExportFunction
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    fn from(other: &NativeFunc<'a, Args, Rets>) -> Self {
        let signature = FunctionType::new(Args::wasm_types(), Rets::wasm_types());
        Self {
            address: other.address,
            vmctx: other.vmctx,
            signature,
            kind: other.arg_kind,
        }
    }
}

impl<'a, Args, Rets> From<NativeFunc<'a, Args, Rets>> for Function
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    fn from(other: NativeFunc<'a, Args, Rets>) -> Self {
        let signature = FunctionType::new(Args::wasm_types(), Rets::wasm_types());
        Self {
            store: other.store,
            definition: other.definition,
            owned_by_store: true, // todo
            exported: ExportFunction {
                address: other.address,
                vmctx: other.vmctx,
                signature,
                kind: other.arg_kind,
            },
        }
    }
}

macro_rules! impl_native_traits {
    (  $( $x:ident ),* ) => {
        #[allow(unused_parens, non_snake_case)]
        impl<'a $( , $x )*, Rets> NativeFunc<'a, ( $( $x, )* ), Rets>
        where
            $( $x: WasmExternType, )*
            Rets: WasmTypeList,
        {
            /// Call the typed func and return results.
            pub fn call(&self, $( $x: $x, )* ) -> Result<Rets, ()> {
                let params = [ $( $x.to_native().to_binary() ),* ];
                let mut values_vec: Vec<i128> = vec![0; std::cmp::max(params.len(), Rets::wasm_types().len())];

                for (i, &arg) in params.iter().enumerate() {
                    values_vec[i] = arg;
                }

                match self.definition {
                    FunctionDefinition::Wasm(WasmFunctionDefinition {
                        trampoline
                    }) => {
                        if let Err(error) = unsafe {
                            wasmer_call_trampoline(
                                self.vmctx,
                                trampoline,
                                self.address,
                                values_vec.as_mut_ptr() as *mut u8,
                            )
                        } {
                            dbg!(error);
                            return Err(());
                        } else {
                            let mut results = Rets::empty_array();
                            let num_results = Rets::wasm_types().len();
                            if num_results > 0 {
                                unsafe {
                                    std::ptr::copy_nonoverlapping(values_vec.as_ptr(),
                                                                  &mut results.as_mut()[0] as *mut i128,
                                                                  num_results);
                                }
                            }
                            return Ok(Rets::from_array(results));
                        }
                    }
                    FunctionDefinition::Host => {
                        /*unsafe {
                            let f = std::mem::transmute::<_, unsafe extern "C" fn( *mut VMContext, $( $x, )*) -> Result<Rets, RuntimeError>>(self.address);
                            match f( self.vmctx, $( $x, )* ) {
                                Err(error) => {
                                    dbg!(error);
                                    return Err(());
                                }
                                Ok(results) => {
                                    return Ok(results);
                                }
                            }
                        }
                        */
                        todo!("host functions not yet implemented")
                    },
                }

            }
        }


    };
}

// impl_native_traits!();
impl_native_traits!();
impl_native_traits!(A1);
impl_native_traits!(A1, A2);
impl_native_traits!(A1, A2, A3);
impl_native_traits!(A1, A2, A3, A4);

// impl_native_traits!(A1, A2, A3);
