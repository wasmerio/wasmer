use crate::export::Export;
use hashbrown::{hash_map::Entry, HashMap, HashSet};
use std::{
    cell::{Ref, RefCell},
    rc::Rc,
};

pub trait LikeNamespace {
    fn get_all_exports(&self) -> HashMap<String, Export>;
    fn get_export(&self, name: &str) -> Option<Export>;
}

pub trait IsExport {
    fn to_export(&self) -> Export;
}

impl IsExport for Export {
    fn to_export(&self) -> Export {
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
/// # use wasmer_runtime_core::{imports, func};
/// # use wasmer_runtime_core::vm::Ctx;
/// let import_object = imports! {
///     "env" => {
///         "foo" => func!(foo),
///     },
/// };
///
/// fn foo(_: &mut Ctx, n: i32) -> i32 {
///     n
/// }
/// ```
pub struct ImportObject {
    map: Rc<RefCell<HashMap<String, Box<dyn LikeNamespace>>>>,
}

impl ImportObject {
    /// Create a new `ImportObject`.
    pub fn new() -> Self {
        Self {
            map: Rc::new(RefCell::new(HashMap::new())),
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
        let mut map = self.map.borrow_mut();

        match map.entry(name.into()) {
            Entry::Vacant(empty) => {
                empty.insert(Box::new(namespace));
                None
            }
            Entry::Occupied(mut occupied) => Some(occupied.insert(Box::new(namespace))),
        }
    }

    pub fn get_namespace(&self, namespace: &str) -> Option<Ref<dyn LikeNamespace + 'static>> {
        let map_ref = self.map.borrow();

        if map_ref.contains_key(namespace) {
            Some(Ref::map(map_ref, |map| &*map[namespace]))
        } else {
            None
        }
    }

    pub fn clone_ref(&self) -> Self {
        Self {
            map: Rc::clone(&self.map),
        }
    }

    pub fn merge(mut imports_a: ImportObject, mut imports_b: ImportObject) -> Self {
        let names_a = imports_a.map.keys();
        let names_b = imports_b.map.keys();
        let names_ab: HashSet<String> = names_a.chain(names_b).cloned().collect();
        let mut merged_imports = ImportObject::new();
        for name in names_ab {
            match (imports_a.map.remove(&name), imports_b.map.remove(&name)) {
                (Some(namespace_a), Some(namespace_b)) => {
                    // Create a combined namespace
                    let mut namespace_ab = Namespace::new();
                    let mut exports_a = namespace_a.get_all_exports();
                    let mut exports_b = namespace_b.get_all_exports();
                    // Import from A will win over B
                    namespace_ab
                        .map
                        .extend(exports_b.drain().map(|(export_name, export)| {
                            (export_name, Box::new(export) as Box<IsExport>)
                        }));
                    namespace_ab
                        .map
                        .extend(exports_a.drain().map(|(export_name, export)| {
                            (export_name, Box::new(export) as Box<IsExport>)
                        }));
                    merged_imports.map.insert(name, Box::new(namespace_ab));
                }
                (Some(namespace_a), None) => {
                    merged_imports.map.insert(name, namespace_a);
                }
                (None, Some(namespace_b)) => {
                    merged_imports.map.insert(name, namespace_b);
                }
                (None, None) => panic!("Unreachable"),
            }
        }
        merged_imports
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
    fn get_all_exports(&self) -> HashMap<String, Export> {
        self.map
            .iter()
            .map(|(name, is_export)| (name.to_string(), is_export.to_export()))
            .collect()
    }

    fn get_export(&self, name: &str) -> Option<Export> {
        self.map.get(name).map(|is_export| is_export.to_export())
    }
}
