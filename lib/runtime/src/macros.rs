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
        };

        Export::Function {
            func: FuncPointer::new($func as _),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![$(Type::$params,)*],
                returns: vec![$(Type::$params,)*],
            },
        }
    }};
    ($func:ident, [ $( $params:ident ),* ]) => {{
        export_func($func, [$($params,)*] -> [])
    }};
}
