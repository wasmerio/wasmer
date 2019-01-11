use crate::{export::Export, Instance};
use hashbrown::{hash_map::Entry, HashMap};

pub trait ImportResolver {
    fn get(&self, namespace: &str, name: &str) -> Option<Export>;
}

enum Namespace {
    Instance(Box<Instance>),
    UserSupplied(HashMap<String, Export>),
}

pub struct Imports {
    map: HashMap<String, Namespace>,
}

impl Imports {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn register_instance(&mut self, namespace: impl Into<String>, instance: Box<Instance>) {
        match self.map.entry(namespace.into()) {
            Entry::Vacant(empty) => empty.insert(Namespace::Instance(instance)),
            Entry::Occupied(_) => {
                panic!("cannot register an instance in a namespace that already exists")
            }
        };
    }

    pub fn register_export(
        &mut self,
        namespace: impl Into<String>,
        name: impl Into<String>,
        export: Export,
    ) {
        let namespace_item = self
            .map
            .entry(namespace.into())
            .or_insert_with(|| Namespace::UserSupplied(HashMap::new()));

        match namespace_item {
            Namespace::UserSupplied(ref mut map) => map.insert(name.into(), export),
            Namespace::Instance(_) => panic!("cannot register an export in a namespace that has already been used to register an instance"),
        };
    }
}

impl ImportResolver for Imports {
    fn get(&self, namespace: &str, name: &str) -> Option<Export> {
        match self.map.get(namespace)? {
            Namespace::UserSupplied(map) => map.get(name).cloned(),
            Namespace::Instance(instance) => instance.get_export(name).ok(),
        }
    }
}
