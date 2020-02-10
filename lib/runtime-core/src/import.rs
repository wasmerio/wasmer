//! The import module contains the implementation data structures and helper functions used to
//! manipulate and access a wasm module's imports including memories, tables, globals, and
//! functions.
use crate::export::Export;
use std::collections::VecDeque;
use std::collections::{hash_map::Entry, HashMap};
use std::{
    borrow::{Borrow, BorrowMut},
    ffi::c_void,
    sync::{Arc, Mutex},
};

/// This trait represents objects that act as a namespace for imports. For example, an `Instance`
/// or `ImportObject` could be considered namespaces that could provide imports to an instance.
pub trait LikeNamespace {
    /// Gets an export by name.
    fn get_export(&self, name: &str) -> Option<Export>;
    /// Gets all exports in the namespace.
    fn get_exports(&self) -> Vec<(String, Export)>;
    /// Maybe insert an `Export` by name into the namespace.
    fn maybe_insert(&mut self, name: &str, export: Export) -> Option<()>;
}

/// A trait that represents `Export` values.
pub trait IsExport {
    /// Gets self as `Export`.
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
    map: Arc<Mutex<HashMap<String, Box<dyn LikeNamespace + Send>>>>,
    pub(crate) state_creator:
        Option<Arc<dyn Fn() -> (*mut c_void, fn(*mut c_void)) + Send + Sync + 'static>>,
    /// Allow missing functions to be generated and instantiation to continue when required
    /// functions are not provided.
    pub allow_missing_functions: bool,
}

impl ImportObject {
    /// Create a new `ImportObject`.
    pub fn new() -> Self {
        Self {
            map: Arc::new(Mutex::new(HashMap::new())),
            state_creator: None,
            allow_missing_functions: false,
        }
    }

    /// Create a new `ImportObject` which generates data from the provided state creator.
    pub fn new_with_data<F>(state_creator: F) -> Self
    where
        F: Fn() -> (*mut c_void, fn(*mut c_void)) + 'static + Send + Sync,
    {
        Self {
            map: Arc::new(Mutex::new(HashMap::new())),
            state_creator: Some(Arc::new(state_creator)),
            allow_missing_functions: false,
        }
    }

    pub(crate) fn call_state_creator(&self) -> Option<(*mut c_void, fn(*mut c_void))> {
        self.state_creator.as_ref().map(|state_gen| state_gen())
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
        N: LikeNamespace + Send + 'static,
    {
        let mut guard = self.map.lock().unwrap();
        let map = guard.borrow_mut();

        match map.entry(name.into()) {
            Entry::Vacant(empty) => {
                empty.insert(Box::new(namespace));
                None
            }
            Entry::Occupied(mut occupied) => Some(occupied.insert(Box::new(namespace))),
        }
    }

    /// Apply a function on the namespace if it exists
    /// If your function can fail, consider using `maybe_with_namespace`
    pub fn with_namespace<Func, InnerRet>(&self, namespace: &str, f: Func) -> Option<InnerRet>
    where
        Func: FnOnce(&(dyn LikeNamespace + Send)) -> InnerRet,
        InnerRet: Sized,
    {
        let guard = self.map.lock().unwrap();
        let map_ref = guard.borrow();
        if map_ref.contains_key(namespace) {
            Some(f(map_ref[namespace].as_ref()))
        } else {
            None
        }
    }

    /// The same as `with_namespace` but takes a function that may fail
    /// # Usage:
    /// ```
    /// # use wasmer_runtime_core::import::{ImportObject, LikeNamespace};
    /// # use wasmer_runtime_core::export::Export;
    /// fn get_export(imports: &ImportObject, namespace: &str, name: &str) -> Option<Export> {
    ///     imports.maybe_with_namespace(namespace, |ns| ns.get_export(name))
    /// }
    /// ```
    pub fn maybe_with_namespace<Func, InnerRet>(&self, namespace: &str, f: Func) -> Option<InnerRet>
    where
        Func: FnOnce(&(dyn LikeNamespace + Send)) -> Option<InnerRet>,
        InnerRet: Sized,
    {
        let guard = self.map.lock().unwrap();
        let map_ref = guard.borrow();
        map_ref
            .get(namespace)
            .map(|ns| ns.as_ref())
            .and_then(|ns| f(ns))
    }

    /// Create a clone ref of this namespace.
    pub fn clone_ref(&self) -> Self {
        Self {
            map: Arc::clone(&self.map),
            state_creator: self.state_creator.clone(),
            allow_missing_functions: false,
        }
    }

    fn get_objects(&self) -> VecDeque<(String, String, Export)> {
        let mut out = VecDeque::new();
        let guard = self.map.lock().unwrap();
        let map = guard.borrow();
        for (name, ns) in map.iter() {
            for (id, exp) in ns.get_exports() {
                out.push_back((name.clone(), id, exp));
            }
        }
        out
    }
}

/// Iterator for an `ImportObject`'s exports.
pub struct ImportObjectIterator {
    elements: VecDeque<(String, String, Export)>,
}

impl Iterator for ImportObjectIterator {
    type Item = (String, String, Export);
    fn next(&mut self) -> Option<Self::Item> {
        self.elements.pop_front()
    }
}

impl IntoIterator for ImportObject {
    type IntoIter = ImportObjectIterator;
    type Item = (String, String, Export);

    fn into_iter(self) -> Self::IntoIter {
        ImportObjectIterator {
            elements: self.get_objects(),
        }
    }
}

impl Extend<(String, String, Export)> for ImportObject {
    fn extend<T: IntoIterator<Item = (String, String, Export)>>(&mut self, iter: T) {
        let mut guard = self.map.lock().unwrap();
        let map = guard.borrow_mut();
        for (ns, id, exp) in iter.into_iter() {
            if let Some(like_ns) = map.get_mut(&ns) {
                like_ns.maybe_insert(&id, exp);
            } else {
                let mut new_ns = Namespace::new();
                new_ns.insert(id, exp);
                map.insert(ns, Box::new(new_ns));
            }
        }
    }
}

/// The top-level container for the two-level wasm imports
pub struct Namespace {
    map: HashMap<String, Box<dyn IsExport + Send>>,
}

impl Namespace {
    /// Create a new empty `Namespace`.
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Insert a new `Export` into the namespace with the given name.
    pub fn insert<S, E>(&mut self, name: S, export: E) -> Option<Box<dyn IsExport + Send>>
    where
        S: Into<String>,
        E: IsExport + Send + 'static,
    {
        self.map.insert(name.into(), Box::new(export))
    }

    /// Returns true if the `Namespace` contains the given name.
    pub fn contains_key<S>(&mut self, key: S) -> bool
    where
        S: Into<String>,
    {
        self.map.contains_key(&key.into())
    }
}

impl LikeNamespace for Namespace {
    fn get_export(&self, name: &str) -> Option<Export> {
        self.map.get(name).map(|is_export| is_export.to_export())
    }

    fn get_exports(&self) -> Vec<(String, Export)> {
        self.map
            .iter()
            .map(|(k, v)| (k.clone(), v.to_export()))
            .collect()
    }

    fn maybe_insert(&mut self, name: &str, export: Export) -> Option<()> {
        self.map.insert(name.to_owned(), Box::new(export));
        Some(())
    }
}

#[cfg(test)]
mod test {
    use crate::export::Export;
    use crate::global::Global;
    use crate::types::Value;

    #[test]
    fn extending_works() {
        let mut imports1 = imports! {
            "dog" => {
                "happy" => Global::new(Value::I32(0)),
            },
        };

        let imports2 = imports! {
            "dog" => {
                "small" => Global::new(Value::I32(2)),
            },
            "cat" => {
                "small" => Global::new(Value::I32(3)),
            },
        };

        imports1.extend(imports2);

        let small_cat_export =
            imports1.maybe_with_namespace("cat", |cat_ns| cat_ns.get_export("small"));
        assert!(small_cat_export.is_some());

        let entries = imports1.maybe_with_namespace("dog", |dog_ns| {
            Some((dog_ns.get_export("happy")?, dog_ns.get_export("small")?))
        });
        assert!(entries.is_some());
    }

    #[test]
    fn extending_conflict_overwrites() {
        let mut imports1 = imports! {
            "dog" => {
                "happy" => Global::new(Value::I32(0)),
            },
        };

        let imports2 = imports! {
            "dog" => {
                "happy" => Global::new(Value::I32(4)),
            },
        };

        imports1.extend(imports2);
        let happy_dog_entry = imports1
            .maybe_with_namespace("dog", |dog_ns| dog_ns.get_export("happy"))
            .unwrap();

        assert!(if let Export::Global(happy_dog_global) = happy_dog_entry {
            happy_dog_global.get() == Value::I32(4)
        } else {
            false
        });
    }
}
