macro_rules! debug {
    ($fmt:expr) => (if cfg!(any(debug_assertions, feature="debug")) { println!(concat!("wasmer-runtime(:{})::", $fmt), line!()) });
    ($fmt:expr, $($arg:tt)*) => (if cfg!(any(debug_assertions, feature="debug")) { println!(concat!("wasmer-runtime(:{})::", $fmt, "\n"), line!(), $($arg)*) });
}

#[macro_export]
macro_rules! export_func {
    ($func:ident, [ $( $params:ident ),* ] -> [ $( $returns:ident ),* ]) => {{
        use wasmer_runtime::{
            export::{Context, Export, FuncPointer},
            types::{FuncSig, Type},
            vm,
        };

        let func: extern fn( $( $crate::__export_func_convert_type!($params), )* &mut vm::Ctx) -> ($( $crate::__export_func_convert_type!($returns) )*) = $func;

        Export::Function {
            func: unsafe { FuncPointer::new(func as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![$(Type::$params,)*],
                returns: vec![$(Type::$params,)*],
            },
        }
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __export_func_convert_type {
    (I32) => {
        i32
    };
    (I64) => {
        i64
    };
    (F32) => {
        f32
    };
    (F64) => {
        f64
    };
}
