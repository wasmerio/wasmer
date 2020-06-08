use crate::new;

pub use new::wasmer::{imports, namespace, ImportObject, ImportObjectIterator, LikeNamespace};

pub struct Namespace {
    exports: new::wasmer::Exports,
}

impl Namespace {
    pub fn new() -> Self {
        Self {
            exports: new::wasmer::Exports::new(),
        }
    }

    pub fn insert<N, V>(&mut self, name: N, value: V)
    where
        N: Into<String>,
        V: Into<new::wasmer::Extern> + Send + 'static,
    {
        self.exports.insert(name, value);
    }

    pub fn contains_key<N>(&mut self, name: N) -> bool
    where
        N: Into<String>,
    {
        self.exports.contains(name)
    }
}

impl LikeNamespace for Namespace {
    fn get_namespace_export(&self, name: &str) -> Option<new::wasmer_runtime::Export> {
        self.exports.get_namespace_export(name)
    }

    fn get_namespace_exports(&self) -> Vec<(String, new::wasmer_runtime::Export)> {
        self.exports.get_namespace_exports()
    }
}

#[deprecated(
    since = "__NEXT_VERSION__",
    note = "Please use the `Exportable` trait instead."
)]
pub trait IsExport {}
