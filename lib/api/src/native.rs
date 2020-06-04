// Native Funcs
// use wasmer_runtime::ExportFunction;
use std::marker::PhantomData;
use wasm_common::{Func, WasmExternType, WasmTypeList};

#[derive(Clone)]
pub struct UnprovidedArgs;
#[derive(Clone)]
pub struct UnprovidedRets;

pub struct NativeFunc<'a, Args = UnprovidedArgs, Rets = UnprovidedRets> {
    // exported: ExportFunction,
    _phantom: PhantomData<(&'a (), Args, Rets)>,
}

unsafe impl<'a, Args, Rets> Send for NativeFunc<'a, Args, Rets> {}

impl<'a, Args, Rets> NativeFunc<'a, Args, Rets> {
    fn from_export() -> Self {
        Self {
            // exported,
            _phantom: PhantomData,
        }
    }
}

/*impl<'a, Args, Rets> From<NativeFunc<'a, Args, Rets>> for Func
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    fn from(other: NativeFunc<'a, Args, Rets>) -> Func {
        Func {

        }
    }
}*/

macro_rules! impl_native_traits {
    (  $( $x:ident ),* ) => {
        #[allow(unused_parens)]
        impl<'a $( , $x )*, Rets> NativeFunc<'a, ( $( $x, )* ), Rets>
        where
            $( $x: WasmExternType, )*
            Rets: WasmTypeList,
        {
            /// Call the typed func and return results.
            pub fn call(&self, $( $x: $x, )* ) -> Result<Rets, ()> {
                unimplemented!("");
            }
        }


    };
}

// impl_native_traits!();
impl_native_traits!(A1);
impl_native_traits!(A1, A2);

// impl_native_traits!(A1, A2, A3);
