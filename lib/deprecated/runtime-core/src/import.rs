use crate::new;

pub use new::wasmer::{imports, ImportObject, ImportObjectIterator, LikeNamespace};

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

#[deprecated(
    since = "__NEXT_VERSION__",
    note = "Please use the `Exportable` trait instead."
)]
pub trait IsExport {}
