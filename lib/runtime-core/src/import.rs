use crate::export::Export;
use hashbrown::{hash_map::Entry, HashMap};
use std::{
    cell::{Ref, RefCell},
    rc::Rc,
};

pub trait LikeNamespace {
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
    fn get_export(&self, name: &str) -> Option<Export> {
        self.map.get(name).map(|is_export| is_export.to_export())
    }
}
