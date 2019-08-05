use crate::export::Export;
use std::collections::VecDeque;
use std::collections::{hash_map::Entry, HashMap};
use std::{
    cell::{Ref, RefCell},
    ffi::c_void,
    rc::Rc,
};

pub trait LikeNamespace {
    fn get_export(&self, name: &str) -> Option<Export>;
    fn get_exports(&self) -> Vec<(String, Export)>;
    fn maybe_insert(&mut self, name: &str, export: Export) -> Option<()>;
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
    pub(crate) state_creator: Option<Rc<Fn() -> (*mut c_void, fn(*mut c_void))>>,
    pub allow_missing_functions: bool,
}

impl ImportObject {
    /// Create a new `ImportObject`.
    pub fn new() -> Self {
        Self {
            map: Rc::new(RefCell::new(HashMap::new())),
            state_creator: None,
            allow_missing_functions: false,
        }
    }

    pub fn new_with_data<F>(state_creator: F) -> Self
    where
        F: Fn() -> (*mut c_void, fn(*mut c_void)) + 'static,
    {
        Self {
            map: Rc::new(RefCell::new(HashMap::new())),
            state_creator: Some(Rc::new(state_creator)),
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
            state_creator: self.state_creator.clone(),
            allow_missing_functions: false,
        }
    }

    fn get_objects(&self) -> VecDeque<(String, String, Export)> {
        let mut out = VecDeque::new();
        for (name, ns) in self.map.borrow().iter() {
            for (id, exp) in ns.get_exports() {
                out.push_back((name.clone(), id, exp));
            }
        }
        out
    }
}

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
        let mut map = self.map.borrow_mut();
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

        let cat_ns = imports1.get_namespace("cat").unwrap();
        assert!(cat_ns.get_export("small").is_some());

        let dog_ns = imports1.get_namespace("dog").unwrap();
        assert!(dog_ns.get_export("happy").is_some());
        assert!(dog_ns.get_export("small").is_some());
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
        let dog_ns = imports1.get_namespace("dog").unwrap();

        assert!(
            if let Export::Global(happy_dog_global) = dog_ns.get_export("happy").unwrap() {
                happy_dog_global.get() == Value::I32(4)
            } else {
                false
            }
        );
    }
}
