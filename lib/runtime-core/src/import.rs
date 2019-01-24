use crate::export::Export;
use hashbrown::{hash_map::Entry, HashMap};

pub trait LikeNamespace {
    fn get_export(&mut self, name: &str) -> Option<Export>;
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
    map: HashMap<String, Export>,
}

impl Namespace {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn insert(&mut self, name: impl Into<String>, export: Export) -> Option<Export> {
        self.map.insert(name.into(), export)
    }
}

impl LikeNamespace for Namespace {
    fn get_export(&mut self, name: &str) -> Option<Export> {
        self.map.get(name).cloned()
    }
}
