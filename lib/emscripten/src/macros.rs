/******************** Mock Function Macro Implementation ********************/

#[macro_export]
macro_rules! mock_external {
    ($namespace:ident, $name:ident, [ $($params:ident),* => $($returns:ident),* ]) => {
        extern "C" fn $name($(_: rust_ty!($params),)*) $(-> rust_ty!($returns))* {
            debug!("emscripten::{} <mock>", stringify!($name));
            $(ret_value!($returns))*
        }

        $namespace.insert(
            stringify!($name),
            Export::Function {
                func: unsafe { FuncPointer::new($name as _) },
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![
                        $(wasmer_ty!($params), )*
                    ],
                    returns: vec![
                        $(wasmer_ty!($returns), )*
                    ]
                },
            },
        );
    };
}

#[macro_export]
macro_rules! wasmer_ty {
    ($ty:ident) => { $ty };
    ($ty0:ident, $($tys:ident),+) => {
        wasmer_ty!($ty0) $(, wasmer_ty!($tys))+
    };
}

#[macro_export]
macro_rules! rust_ty {
    (I32) => { i32 };
    (I64) => { i64 };
    (F32) => { f32 };
    (F64) => { f64 };
    ($ty0:ident, $($tys:ident),+) => {
        rust_ty!($ty0) $(, rust_ty!($tys))+
    };
}

#[macro_export]
macro_rules! ret_value {
    (I32) => {
        -1
    };
    (I64) => {
        -1
    };
    (F32) => {
        -1.0
    };
    (F64) => {
        -1.0
    };
}

/******************** Export Pointers ********************/

#[macro_export]
macro_rules! func {
    ($namespace:ident, $function:ident) => {{
        unsafe { FuncPointer::new($namespace::$function as _) }
    }};
}

#[macro_export]
macro_rules! global {
    ($value:ident) => {{
        unsafe {
            GlobalPointer::new(
                // NOTE: Taking a shortcut here. LocalGlobal is a struct containing just u64.
                std::mem::transmute::<&u64, *mut LocalGlobal>($value),
            )
        }
    }};
}

/******************** Debug ********************/

#[macro_export]
macro_rules! debug {
    ($fmt:expr) => (if cfg!(any(debug_assertions, feature="debug")) { println!(concat!("wasmer-emscripten(:{})::", $fmt), line!()) });
    ($fmt:expr, $($arg:tt)*) => (if cfg!(any(debug_assertions, feature="debug")) { println!(concat!("wasmer-emscripten(:{})::", $fmt, "\n"), line!(), $($arg)*) });
}
