#![allow(unused)]

macro_rules! debug {
    ($fmt:expr) => (if cfg!(any(debug_assertions, feature="debug")) { println!(concat!("wasmer-runtime(:{})::", $fmt), line!()) });
    ($fmt:expr, $($arg:tt)*) => (if cfg!(any(debug_assertions, feature="debug")) { println!(concat!("wasmer-runtime(:{})::", $fmt, "\n"), line!(), $($arg)*) });
}

#[macro_export]
macro_rules! func {
    ($func:ident, [ $( $params:ident ),* ] -> [ $( $returns:ident ),* ] ) => {{
        use $crate::{
            export::{Context, Export, FuncPointer},
            types::{FuncSig, Type},
            vm,
        };

        let func: extern fn( $( $params, )* &mut vm::Ctx) -> ($( $returns )*) = $func;

        Export::Function {
            func: unsafe { FuncPointer::new(func as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![$($crate::__export_func_convert_type!($params),)*],
                returns: vec![$($crate::__export_func_convert_type!($returns),)*],
            },
        }
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __export_func_convert_type {
    (i32) => {
        Type::I32
    };
    (u32) => {
        Type::I32
    };
    (i64) => {
        Type::I64
    };
    (u64) => {
        Type::I32
    };
    (f32) => {
        Type::F32
    };
    (f64) => {
        Type::F64
    };
    ($x:ty) => {
        compile_error!("Only `i32`, `u32`, `i64`, `u64`, `f32`, and `f64` are supported for argument and return types")
    };
}

/// Generate an [`ImportObject`] safely.
///
/// [`ImportObject`]: struct.ImportObject.html
///
/// # Note:
/// The `import` macro currently only supports
/// importing functions.
///
///
/// # Usage:
/// ```
/// # use wasmer_runtime_core::imports;
/// # use wasmer_runtime_core::vm::Ctx;
/// let import_object = imports! {
///     "env" => {
///         "foo" => foo<[i32] -> [i32]>,
///     },
/// };
///
/// extern fn foo(n: i32, _: &mut Ctx) -> i32 {
///     n
/// }
/// ```
#[macro_export]
macro_rules! imports {
    ( $( $ns_name:expr => $ns:tt, )* ) => {{
        use $crate::{
            import::{ImportObject, Namespace},
        };

        let mut import_object = ImportObject::new();

        $({
            let ns = $crate::__imports_internal!($ns);

            import_object.register($ns_name, ns);
        })*

        import_object
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __imports_internal {
    ( { $( $imp_name:expr => $import_item:expr, )* } ) => {{
        let mut ns = Namespace::new();
        $(
            ns.insert($imp_name, $import_item);
        )*
        ns
    }};
    ($ns:ident) => {
        $ns
    };
}
