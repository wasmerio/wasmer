use crate::export::Export;
use hashbrown::{hash_map::Entry, HashMap};

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

    pub fn register<S, N>(&mut self, name: S, namespace: N) -> Option<Box<dyn Namespace>>
    where
        S: Into<String>,
        N: Namespace + 'static,
    {
        match self.map.entry(name.into()) {
            Entry::Vacant(empty) => {
                empty.insert(Box::new(namespace));
                None
            }
            Entry::Occupied(mut occupied) => Some(occupied.insert(Box::new(namespace))),
        }
    }

    pub fn get_namespace(&self, namespace: &str) -> Option<&dyn Namespace> {
        self.map.get(namespace).map(|namespace| &**namespace)
    }
}
