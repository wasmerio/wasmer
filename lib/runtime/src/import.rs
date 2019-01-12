use crate::export::Export;
use hashbrown::{hash_map::Entry, HashMap};

pub trait ImportResolver {
    fn get(&self, namespace: &str, name: &str) -> Option<Export>;
}

pub trait Namespace {
    fn get_export(&self, name: &str) -> Option<Export>;
}

impl Namespace for HashMap<String, Export> {
    fn get_export(&self, name: &str) -> Option<Export> {
        self.get(name).cloned()
    }
}
impl<'a> Namespace for HashMap<&'a str, Export> {
    fn get_export(&self, name: &str) -> Option<Export> {
        self.get(name).cloned()
    }
}

pub struct Imports {
    map: HashMap<String, Box<dyn Namespace>>,
}

impl Imports {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn register(
        &mut self,
        name: impl Into<String>,
        namespace: impl Namespace + 'static,
    ) -> Option<Box<dyn Namespace>> {
        match self.map.entry(name.into()) {
            Entry::Vacant(empty) => {
                empty.insert(Box::new(namespace));
                None
            }
            Entry::Occupied(mut occupied) => Some(occupied.insert(Box::new(namespace))),
        }
    }

    // pub fn register_instance(&mut self, namespace: impl Into<String>, instance: Box<Instance>) {
    //     match self.map.entry(namespace.into()) {
    //         Entry::Vacant(empty) => empty.insert(Namespace::Instance(instance)),
    //         Entry::Occupied(_) => {
    //             panic!("cannot register an instance in a namespace that already exists")
    //         }
    //     };
    // }

    // pub fn register_export(
    //     &mut self,
    //     namespace: impl Into<String>,
    //     name: impl Into<String>,
    //     export: Export,
    // ) {
    //     let namespace_item = self
    //         .map
    //         .entry(namespace.into())
    //         .or_insert_with(|| Namespace::UserSupplied(HashMap::new()));

    //     match namespace_item {
    //         Namespace::UserSupplied(ref mut map) => map.insert(name.into(), export),
    //         Namespace::Instance(_) => panic!("cannot register an export in a namespace that has already been used to register an instance"),
    //     };
    // }
}

impl ImportResolver for Imports {
    fn get(&self, namespace_name: &str, name: &str) -> Option<Export> {
        let namespace = self.map.get(namespace_name)?;
        namespace.get_export(name)
    }
}
