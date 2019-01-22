use crate::export::Export;
use hashbrown::{hash_map::Entry, HashMap};

pub trait LikeNamespace {
    fn get_export(&mut self, name: &str) -> Option<Export>;
}

pub struct ImportObject {
    map: HashMap<String, Box<dyn LikeNamespace>>,
}

impl ImportObject {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

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
