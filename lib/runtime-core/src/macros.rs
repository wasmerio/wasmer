/// Prints a log message with args, similar to println, when the debug feature is enabled.
/// If the debug feature is disabled, arguments are not evaluated or printed.
#[macro_export]
#[cfg(feature = "debug")]
macro_rules! debug {
    ($fmt:expr) => (println!(concat!("[{}] wasmer-runtime(:{}) ", $fmt), {
       let time = ::std::time::SystemTime::now().duration_since(::std::time::UNIX_EPOCH).expect("Can't get time");
       format!("{}.{:03}", time.as_secs(), time.subsec_millis())
    }, line!()));
    ($fmt:expr, $($arg:tt)*) => (println!(concat!("[{}] wasmer-runtime(:{}) ", $fmt, "\n"), {
       let time = ::std::time::SystemTime::now().duration_since(::std::time::UNIX_EPOCH).expect("Can't get time");
       format!("{}.{:03}", time.as_secs(), time.subsec_millis())
    }, line!(), $($arg)*));
}

/// Prints a log message with args, similar to println, when the debug feature is enabled.
/// If the debug feature is disabled, arguments are not evaluated or printed.
#[macro_export]
#[cfg(not(feature = "debug"))]
macro_rules! debug {
    ($fmt:expr) => {};
    ($fmt:expr, $($arg:tt)*) => {};
}

/// Prints a log message with args, similar to println, when the trace feature is enabled.
/// If the trace feature is disabled, arguments are not evaluated or printed.
#[macro_export]
#[cfg(feature = "trace")]
macro_rules! trace {
    ($fmt:expr) => {
        debug!($fmt)
    };
    ($fmt:expr, $($arg:tt)*) => {
        debug!($fmt, $($arg)*);
    }
}

/// Prints a log message with args, similar to println, when the trace feature is enabled.
/// If the trace feature is disabled, arguments are not evaluated or printed.
#[macro_export]
#[cfg(not(feature = "trace"))]
macro_rules! trace {
    ($fmt:expr) => {};
    ($fmt:expr, $($arg:tt)*) => {};
}

/// Helper macro to create a new `Func` object using the provided function pointer.
///
/// # Usage
///
/// Function pointers or closures are supported. Closures can capture
/// their environment (with `move`). The first parameter is likely to
/// be of kind `vm::Ctx`, though it can be optional.
///
/// ```
/// # use wasmer_runtime_core::{imports, func};
/// # use wasmer_runtime_core::vm;
///
/// // A function that has access to `vm::Ctx`.
/// fn func_with_vmctx(_: &mut vm::Ctx, n: i32) -> i32 {
///     n
/// }
///
/// // A function that cannot access `vm::Ctx`.
/// fn func(n: i32) -> i32 {
///     n
/// }
///
/// let i = 7;
///
/// let import_object = imports! {
///     "env" => {
///         "foo" => func!(func_with_vmctx),
///         "bar" => func!(func),
///         // A closure with a captured environment, and an access to `vm::Ctx`.
///         "baz" => func!(move |_: &mut vm::Ctx, n: i32| -> i32 {
///             n + i
///         }),
///         // A closure without a captured environment, and no access to `vm::Ctx`.
///         "qux" => func!(|n: i32| -> i32 {
///             n
///         }),
///     },
/// };
/// ```
#[macro_export]
macro_rules! func {
    ($func:expr) => {{
        $crate::Func::new($func)
    }};
}

/// Generate an [`ImportObject`] safely.
///
/// [`ImportObject`]: struct.ImportObject.html
///
/// # Note
/// The `import` macro currently only supports
/// importing functions.
///
///
/// # Usage
/// ```
/// # use wasmer_runtime_core::{imports, func};
/// # use wasmer_runtime_core::vm::Ctx;
/// let import_object = imports! {
///     "env" => {
///         "foo" => func!(foo),
///     },
/// };
///
/// let imports_with_state = imports! {
///     || (0 as _, |_a| {}),
///     "env" => {
///         "foo" => func!(foo),
///     },
/// };
///
/// fn foo(_: &mut Ctx, n: i32) -> i32 {
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
    ($state_gen:expr, $( $ns_name:expr => $ns:tt, )* ) => {{
        use $crate::{
            import::{ImportObject, Namespace},
        };

        let mut import_object = ImportObject::new_with_data($state_gen);

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

#[macro_export]
#[doc(hidden)]
macro_rules! namespace {
    ( $( $imp_name:expr => $import_item:expr, )* ) => {{
        let mut ns = $crate::import::Namespace::new();
        $(
            ns.insert($imp_name, $import_item);
        )*
        ns
    }};
}
