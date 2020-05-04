use crate::externals::{Extern, Function, Global, Memory, Table};
use crate::import_object::LikeNamespace;
use indexmap::IndexMap;
use std::iter::FromIterator;
use std::sync::Arc;
use thiserror::Error;
use wasmer_runtime::Export;

/// The `ExportError` can happen when trying to get a specific
/// export [`Extern`] from the [`Instance`] exports.
///
/// [`Instance`]: crate::Instance
///
/// ```ignore
/// # let my_instance = Instance::new(...);
///
/// // This results with an error: `ExportError::IncompatibleType`.
/// let missing_import: &Global = my_instance.exports.get("func")?;
///
/// // This results with an error: `ExportError::Missing`.
/// let missing_import: &Func = my_instance.exports.get("unknown")?;
/// ```
#[derive(Error, Debug)]
pub enum ExportError {
    /// An error than occurs when the exported type and the expected type
    /// are incompatible.
    #[error("Incompatible Export Type")]
    IncompatibleType,
    /// This error arises when an export is missing
    #[error("Missing export {0}")]
    Missing(String),
}

/// Exports is a special kind of map that allows easily unwrapping
/// the types of instances.
#[derive(Clone)]
pub struct Exports {
    map: Arc<IndexMap<String, Extern>>,
}

impl Exports {
    /// Creates a new `Exports`.
    pub fn new() -> Self {
        Exports {
            map: Arc::new(IndexMap::new()),
        }
    }

    /// Creates a new `Exports` with capacity `n`.
    pub fn with_capacity(n: usize) -> Self {
        Exports {
            map: Arc::new(IndexMap::with_capacity(n)),
        }
    }

    /// Return the number of exports in the `Exports` map.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Insert a new export into this `Exports` map.
    pub fn insert<S, E>(&mut self, name: S, value: E)
    where
        S: Into<String>,
        E: Into<Extern>,
    {
        Arc::get_mut(&mut self.map)
            .unwrap()
            .insert(name.into(), value.into());
    }

    /// Get an export given a `name`.
    ///
    /// The `get` method is specifically made for usage inside of
    /// Rust APIs, as we can detect what's the desired type easily.
    ///
    /// If you want to get an export dynamically with type checking
    /// please use the following functions: `get_func`, `get_memory`,
    /// `get_table` or `get_global` instead.
    ///
    /// If you want to get an export dynamically handling manually
    /// type checking manually, please use `get_extern`.
    pub fn get<'a, T: Exportable<'a>>(&'a self, name: &str) -> Result<&T, ExportError> {
        match self.map.get(name) {
            None => return Err(ExportError::Missing(name.to_string())),
            Some(extern_) => T::get_self_from_extern(extern_),
        }
    }

    /// Get an export as a `Global`.
    pub fn get_global(&self, name: &str) -> Result<&Global, ExportError> {
        self.get(name)
    }

    /// Get an export as a `Memory`.
    pub fn get_memory(&self, name: &str) -> Result<&Memory, ExportError> {
        self.get(name)
    }

    /// Get an export as a `Table`.
    pub fn get_table(&self, name: &str) -> Result<&Table, ExportError> {
        self.get(name)
    }

    /// Get an export as a `Func`.
    pub fn get_function(&self, name: &str) -> Result<&Function, ExportError> {
        self.get(name)
    }

    /// Get an export as an `Extern`.
    pub fn get_extern(&self, name: &str) -> Option<&Extern> {
        self.map.get(name)
    }

    /// Returns true if the `Exports` contains the given name.
    pub fn contains<S>(&mut self, name: S) -> bool
    where
        S: Into<String>,
    {
        Arc::get_mut(&mut self.map)
            .unwrap()
            .contains_key(&name.into())
    }
}

impl FromIterator<(String, Extern)> for Exports {
    fn from_iter<I: IntoIterator<Item = (String, Extern)>>(iter: I) -> Self {
        // TODO: Move into IndexMap collect
        let mut exports = Exports::new();
        for (name, extern_) in iter {
            exports.insert(name, extern_);
        }
        exports
    }
}

impl LikeNamespace for Exports {
    fn get_namespace_export(&self, name: &str) -> Option<Export> {
        self.map.get(name).map(|is_export| is_export.to_export())
    }

    fn get_namespace_exports(&self) -> Vec<(String, Export)> {
        self.map
            .iter()
            .map(|(k, v)| (k.clone(), v.to_export()))
            .collect()
    }
}

/// This trait is used to mark types as gettable from an [`Instance`].
///
/// [`Instance`]: crate::Instance
pub trait Exportable<'a>: Sized {
    /// This function is used when providedd the [`Extern`] as exportable, so it
    /// can be used while instantiating the [`Module`].
    ///
    /// [`Module`]: crate::Module
    fn to_export(&self) -> Export;

    /// Implementation of how to get the export corresponding to the implementing type
    /// from an [`Instance`] by name.
    ///
    /// [`Instance`]: crate::Instance
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError>;
}
