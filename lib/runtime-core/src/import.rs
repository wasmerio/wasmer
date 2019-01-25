use crate::export::Export;
use hashbrown::{hash_map::Entry, HashMap};

pub trait LikeNamespace {
    fn get_export(&mut self, name: &str) -> Option<Export>;
}

pub trait IsExport {
    fn to_export(&mut self) -> Export;
}

impl IsExport for Export {
    fn to_export(&mut self) -> Export {
        self.clone()
    }
}

/// All of the import data used when instantiating.
///
/// It's suggested that you use the [`imports!`] macro
/// instead of creating an `ImportObject` by hand.
///
/// [`imports!`]: macro.imports.html
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
pub struct ImportObject {
    map: HashMap<String, Box<dyn LikeNamespace>>,
}

impl ImportObject {
    /// Create a new `ImportObject`.  
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Register anything that implements `LikeNamespace` as a namespace.
    ///
    /// # Usage:
    /// ```
    /// # use wasmer_runtime_core::Instance;
    /// # use wasmer_runtime_core::import::{ImportObject, Namespace};
    /// fn register(instance: Instance, namespace: Namespace) {
    ///     let mut import_object = ImportObject::new();
    ///
    ///     import_object.register("namespace0", instance);
    ///     import_object.register("namespace1", namespace);
    ///     // ...
    /// }
    /// ```
    pub fn register<S, N>(&mut self, name: S, namespace: N) -> Option<Box<dyn LikeNamespace>>
    where
        S: Into<String>,
        N: LikeNamespace + 'static,
    {
        match self.map.entry(name.into()) {
            Entry::Vacant(empty) => {
                empty.insert(Box::new(namespace));
                None
            }
            Entry::Occupied(mut occupied) => Some(occupied.insert(Box::new(namespace))),
        }
    }

    pub fn get_namespace(&mut self, namespace: &str) -> Option<&mut (dyn LikeNamespace + 'static)> {
        self.map
            .get_mut(namespace)
            .map(|namespace| &mut **namespace)
    }
}

pub struct Namespace {
    map: HashMap<String, Box<dyn IsExport>>,
}

impl Namespace {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn insert<S, E>(&mut self, name: S, export: E) -> Option<Box<dyn IsExport>>
    where
        S: Into<String>,
        E: IsExport + 'static,
    {
        self.map.insert(name.into(), Box::new(export))
    }
}

impl LikeNamespace for Namespace {
    fn get_export(&mut self, name: &str) -> Option<Export> {
        self.map
            .get_mut(name)
            .map(|is_export| is_export.to_export())
    }
}
