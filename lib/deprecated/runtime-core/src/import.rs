use crate::new;

pub use new::wasmer::{namespace, ImportObject, ImportObjectIterator, LikeNamespace};

pub struct Namespace {
    exports: new::wasmer::Exports,
}

impl Namespace {
    pub fn new() -> Self {
        Self {
            exports: new::wasmer::Exports::new(),
        }
    }

    pub fn insert<N, V>(&mut self, name: N, value: V)
    where
        N: Into<String>,
        V: Into<new::wasmer::Extern> + Send + 'static,
    {
        self.exports.insert(name, value);
    }

    pub fn contains_key<N>(&mut self, name: N) -> bool
    where
        N: Into<String>,
    {
        self.exports.contains(name)
    }
}

impl LikeNamespace for Namespace {
    fn get_namespace_export(&self, name: &str) -> Option<new::wasmer_runtime::Export> {
        self.exports.get_namespace_export(name)
    }

    fn get_namespace_exports(&self) -> Vec<(String, new::wasmer_runtime::Export)> {
        self.exports.get_namespace_exports()
    }
}

#[deprecated(
    since = "__NEXT_VERSION__",
    note = "Please use the `Exportable` trait instead."
)]
pub trait IsExport {}

/// Generate an `ImportObject` easily with the `imports!` macro.
///
/// # Usage
///
/// ```
/// # use wasmer_runtime_core::{imports, func};
///
/// let import_object = imports! {
///     "env" => {
///         "foo" => func!(foo)
///     },
/// };
///
/// fn foo(n: i32) -> i32 {
///     n
/// }
/// ```
///
/// or by passing a state creator for the import object:
///
/// ```
/// # use wasmer_runtime_core::{imports, func};
///
/// let import_object = imports! {
///     || (0 as _, |_a| {}),
///     "env" => {
///         "foo" => func!(foo)
///     },
/// };
///
/// # fn foo(n: i32) -> i32 {
/// #     n
/// # }
/// ```
#[macro_export]
macro_rules! imports {
    ( $( $namespace_name:expr => $namespace:tt ),* $(,)? ) => {
        {
            let mut import_object = $crate::import::ImportObject::new();

            $({
                let namespace = $crate::import_namespace!($namespace);

                import_object.register($namespace_name, namespace);
            })*

            import_object
        }
    };

    ($state_creator:expr, $( $namespace_name:expr => $namespace:tt ),* $(,)? ) => {
        {
            let mut import_object = $crate::import::ImportObject::new_with_data($state_creator);

            $({
                let namespace = $crate::import_namespace!($namespace);

                import_object.register($namespace_name, namespace);
            })*

            import_object
        }
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! import_namespace {
    ( { $( $import_name:expr => $import_item:expr ),* $(,)? } ) => {
        {
            let mut namespace = $crate::instance::Exports::new();

            $(
                namespace.insert($import_name, $import_item);
            )*

            namespace
        }
    };

    ($ns:ident) => {
        $ns
    };
}
