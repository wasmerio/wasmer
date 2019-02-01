#[macro_export]
#[cfg(feature = "debug")]
macro_rules! debug {
    ($fmt:expr) => (println!(concat!("wasmer-runtime(:{})::", $fmt), line!()));
    ($fmt:expr, $($arg:tt)*) => (println!(concat!("wasmer-runtime(:{})::", $fmt, "\n"), line!(), $($arg)*));
}

#[macro_export]
#[cfg(not(feature = "debug"))]
macro_rules! debug {
    ($fmt:expr) => {};
    ($fmt:expr, $($arg:tt)*) => {};
}

#[macro_export]
macro_rules! func {
    ($func:path, [ $( $params:ident ),* ] -> [ $( $returns:ident ),* ] ) => {{
        use $crate::{
            export::{Context, Export, FuncPointer},
            types::{FuncSig, Type, WasmExternType},
            vm,
        };
        let func: extern fn( $( $params, )* &mut vm::Ctx) -> ($( $returns )*) = $func;
        Export::Function {
            func: unsafe { FuncPointer::new(func as _) },
            ctx: Context::Internal,
            signature: FuncSig::new(
                &[ $( <$params as WasmExternType>::TYPE, )* ] as &[Type],
                &[ $( <$returns as WasmExternType>::TYPE, )* ] as &[Type],
            ).into(),
        }
    }};
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
/// # use wasmer_runtime_core::{imports, func};
/// # use wasmer_runtime_core::vm::Ctx;
/// let import_object = imports! {
///     "env" => {
///         "foo" => func!(foo, [i32] -> [i32]),
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
