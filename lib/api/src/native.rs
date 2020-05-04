// Native Funcs
// use wasmer_runtime::ExportFunction;
use std::marker::PhantomData;
use wasm_common::{WasmExternType, WasmTypeList};
// pub trait NativeHostFunction<Args, Rets>
// where
//     Args: WasmTypeList,
//     Rets: WasmTypeList,
// {
// }

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

// #[allow (unused_parens)]
// impl <'a, A1, Rets > NativeFunc <'a, (A1,), Rets >
// where
//     A1 : WasmExternType,
//     Rets : WasmTypeList,
// {
//     /// Call the typed func and return results.
//     pub fn calla(&self, A1:A1,) -> Result <Rets, ()>{
//        unimplemented!("");
//     }
// }

// #[allow (unused_parens)]
// impl <'a, A1, A2, Rets > NativeFunc <'a, (A1, A2), Rets >
// where
//     A1 : WasmExternType,
//     A2 : WasmExternType,
//     Rets : WasmTypeList,
// {
//     /// Call the typed func and return results.
//     pub fn calla(&self, A1:A1, A2: A2,) -> Result <Rets, ()>{
//        unimplemented!("");
//     }
// }

macro_rules! impl_native_traits {
    (  $( $x:ident ),* ) => {
        #[allow(unused_parens)]
        impl<'a $( , $x )*, Rets> NativeFunc<'a, ( $( $x ),* ), Rets>
        where
            $( $x: WasmExternType, )*
            Rets: WasmTypeList,
        {
            /// Call the typed func and return results.
            pub fn call(&self, $( $x: $x, )* ) -> Result<Rets, ()> {
                trace_macros!(false);
                unimplemented!("");
                trace_macros!(true);
            }
        }
        // impl<'a $( , $x )*, Rets> NativeFunc<'a, ( $( $x ),* ), Rets>
        // where
        //     $( $x: WasmExternType, )*
        //     Rets: WasmTypeList,
        // {
        //     /// Call the typed func and return results.
        //     #[allow(non_snake_case, clippy::too_many_arguments)]
        //     pub fn call(&self, $( $x: $x, )* ) -> Result<Rets::CStruct, RuntimeError> {
        //         unimplemented!("");
        //         // use std::mem;
        //         // use std::cell::RefCell;
        //         // let vmctx = self.exported.vmctx;
        //         // let callee_address = self.exported.address;
        //         // #[allow(unused_parens)]
        //         // unsafe {
        //         //     let result: RefCell<Option<Rets::CStruct>> = RefCell::new(None);
        //         //     catch_traps(vmctx, || {
        //         //         let func_result = mem::transmute::<
        //         //             *const VMFunctionBody,
        //         //             unsafe extern "C" fn(*mut VMContext, *mut VMContext, $( $x ),* ) -> Rets::CStruct,
        //         //         >(callee_address)(vmctx, std::ptr::null_mut() as *mut VMContext, $( $x ),* );

        //         //         *result.borrow_mut() = Some(func_result);
        //         //     });
        //         //     Ok(result.into_inner().unwrap())
        //         // }
        //     }
        // }


    };
}

trace_macros!(true);

// impl_native_traits!();
impl_native_traits!(A1);
impl_native_traits!(A1, A2);

trace_macros!(false);

// impl_native_traits!(A1, A2, A3);
