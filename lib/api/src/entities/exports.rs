use crate::store::AsStoreRef;
use crate::{Extern, Function, Global, Memory, Table, TypedFunction, WasmTypeList};
use indexmap::IndexMap;
use std::fmt;
use std::iter::{ExactSizeIterator, FromIterator};
use thiserror::Error;

/// The `ExportError` can happen when trying to get a specific
/// export [`Extern`] from the [`Instance`] exports.
///
/// [`Instance`]: crate::Instance
///
/// # Examples
///
/// ## Incompatible export type
///
/// ```should_panic
/// # use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, Type, Value, ExportError};
/// # let mut store = Store::default();
/// # let wasm_bytes = wat2wasm(r#"
/// # (module
/// #   (global $one (export "glob") f32 (f32.const 1)))
/// # "#.as_bytes()).unwrap();
/// # let module = Module::new(&store, wasm_bytes).unwrap();
/// # let import_object = imports! {};
/// # let instance = Instance::new(&mut store, &module, &import_object).unwrap();
/// #
/// // This results with an error: `ExportError::IncompatibleType`.
/// let export = instance.exports.get_function("glob").unwrap();
/// ```
///
/// ## Missing export
///
/// ```should_panic
/// # use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, Type, Value, ExportError};
/// # let mut store = Store::default();
/// # let wasm_bytes = wat2wasm("(module)".as_bytes()).unwrap();
/// # let module = Module::new(&store, wasm_bytes).unwrap();
/// # let import_object = imports! {};
/// # let instance = Instance::new(&mut store, &module, &import_object).unwrap();
/// #
/// // This results with an error: `ExportError::Missing`.
/// let export = instance.exports.get_function("unknown").unwrap();
/// ```
#[derive(Error, Debug, Clone)]
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
///
/// TODO: add examples of using exports
#[derive(Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct Exports {
    map: IndexMap<String, Extern>,
}

impl Exports {
    /// Creates a new `Exports`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Creates a new `Exports` with capacity `n`.
    pub fn with_capacity(n: usize) -> Self {
        Self {
            map: IndexMap::with_capacity(n),
        }
    }

    /// Return the number of exports in the `Exports` map.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Return whether or not there are no exports
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Insert a new export into this `Exports` map.
    pub fn insert<S, E>(&mut self, name: S, value: E)
    where
        S: Into<String>,
        E: Into<Extern>,
    {
        self.map.insert(name.into(), value.into());
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
    pub fn get<'a, T: Exportable<'a>>(&'a self, name: &str) -> Result<&'a T, ExportError> {
        match self.map.get(name) {
            None => Err(ExportError::Missing(name.to_string())),
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

    /// Get an export as a `TypedFunction`.
    pub fn get_typed_function<Args, Rets>(
        &self,
        store: &impl AsStoreRef,
        name: &str,
    ) -> Result<TypedFunction<Args, Rets>, ExportError>
    where
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        self.get_function(name)?
            .typed(store)
            .map_err(|_| ExportError::IncompatibleType)
    }

    /// Hack to get this working with nativefunc too
    pub fn get_with_generics<'a, T, Args, Rets>(&'a self, name: &str) -> Result<T, ExportError>
    where
        Args: WasmTypeList,
        Rets: WasmTypeList,
        T: ExportableWithGenerics<'a, Args, Rets>,
    {
        match self.map.get(name) {
            None => Err(ExportError::Missing(name.to_string())),
            Some(extern_) => T::get_self_from_extern_with_generics(extern_),
        }
    }

    /// Get an export as an `Extern`.
    pub fn get_extern(&self, name: &str) -> Option<&Extern> {
        self.map.get(name)
    }

    /// Returns true if the `Exports` contains the given export name.
    pub fn contains<S>(&self, name: S) -> bool
    where
        S: Into<String>,
    {
        self.map.contains_key(&name.into())
    }

    /// Get an iterator over the exports.
    pub fn iter(&self) -> ExportsIterator<'_, indexmap::map::Iter<'_, String, Extern>> {
        ExportsIterator {
            iter: self.map.iter(),
        }
    }
}

impl fmt::Debug for Exports {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

/// An iterator over exports.
pub struct ExportsIterator<'a, I>
where
    I: Iterator<Item = (&'a String, &'a Extern)> + Sized,
{
    iter: I,
}

impl<'a, I> Iterator for ExportsIterator<'a, I>
where
    I: Iterator<Item = (&'a String, &'a Extern)> + Sized,
{
    type Item = (&'a String, &'a Extern);

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

impl<'a, I> ExactSizeIterator for ExportsIterator<'a, I>
where
    I: Iterator<Item = (&'a String, &'a Extern)> + ExactSizeIterator + Sized,
{
    fn len(&self) -> usize {
        self.iter.len()
    }
}

impl<'a, I> ExportsIterator<'a, I>
where
    I: Iterator<Item = (&'a String, &'a Extern)> + Sized,
{
    /// Get only the functions.
    pub fn functions(self) -> impl Iterator<Item = (&'a String, &'a Function)> + Sized {
        self.iter.filter_map(|(name, export)| match export {
            Extern::Function(function) => Some((name, function)),
            _ => None,
        })
    }

    /// Get only the memories.
    pub fn memories(self) -> impl Iterator<Item = (&'a String, &'a Memory)> + Sized {
        self.iter.filter_map(|(name, export)| match export {
            Extern::Memory(memory) => Some((name, memory)),
            _ => None,
        })
    }

    /// Get only the globals.
    pub fn globals(self) -> impl Iterator<Item = (&'a String, &'a Global)> + Sized {
        self.iter.filter_map(|(name, export)| match export {
            Extern::Global(global) => Some((name, global)),
            _ => None,
        })
    }

    /// Get only the tables.
    pub fn tables(self) -> impl Iterator<Item = (&'a String, &'a Table)> + Sized {
        self.iter.filter_map(|(name, export)| match export {
            Extern::Table(table) => Some((name, table)),
            _ => None,
        })
    }
}

impl FromIterator<(String, Extern)> for Exports {
    fn from_iter<I: IntoIterator<Item = (String, Extern)>>(iter: I) -> Self {
        Self {
            map: IndexMap::from_iter(iter),
        }
    }
}

impl IntoIterator for Exports {
    type IntoIter = indexmap::map::IntoIter<String, Extern>;
    type Item = (String, Extern);

    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}

impl<'a> IntoIterator for &'a Exports {
    type IntoIter = indexmap::map::Iter<'a, String, Extern>;
    type Item = (&'a String, &'a Extern);

    fn into_iter(self) -> Self::IntoIter {
        self.map.iter()
    }
}

/// This trait is used to mark types as gettable from an [`Instance`].
///
/// [`Instance`]: crate::Instance
pub trait Exportable<'a>: Sized {
    /// Implementation of how to get the export corresponding to the implementing type
    /// from an [`Instance`] by name.
    ///
    /// [`Instance`]: crate::Instance
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError>;
}

/// A trait for accessing exports (like [`Exportable`]) but it takes generic
/// `Args` and `Rets` parameters so that `TypedFunction` can be accessed directly
/// as well.
pub trait ExportableWithGenerics<'a, Args: WasmTypeList, Rets: WasmTypeList>: Sized {
    /// Get an export with the given generics.
    fn get_self_from_extern_with_generics(_extern: &'a Extern) -> Result<Self, ExportError>;
}

/// We implement it for all concrete [`Exportable`] types (that are `Clone`)
/// with empty `Args` and `Rets`.
impl<'a, T: Exportable<'a> + Clone + 'static> ExportableWithGenerics<'a, (), ()> for T {
    fn get_self_from_extern_with_generics(_extern: &'a Extern) -> Result<Self, ExportError> {
        T::get_self_from_extern(_extern).cloned()
    }
}
