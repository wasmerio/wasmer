//! Ordered Resolvers are a custom kind of [`Resolver`] that retrieves
//! `EngineExport`s based on the index of the import, and not the module or name.
//!
//! This resolver is used in the Wasm-C-API as the imports are provided
//! by index and not by module and name.

use std::iter::FromIterator;
use wasmer_api::{Export, Exportable, Extern, Resolver};

/// An `OrderedResolver` stores all the `externs` provided to an Instance
/// in a Vec, so we can retrieve them later based on index.
#[derive(Clone)]
pub struct OrderedResolver {
    /// The externs to be resolved by index
    externs: Vec<Extern>,
}

impl Resolver for OrderedResolver {
    fn resolve(&self, index: u32, _module: &str, _name: &str) -> Option<Export> {
        self.externs
            .get(index as usize)
            .map(|extern_| extern_.to_export())
    }
}

impl FromIterator<Extern> for OrderedResolver {
    fn from_iter<I: IntoIterator<Item = Extern>>(iter: I) -> Self {
        let mut externs = Vec::new();
        for extern_ in iter {
            externs.push(extern_);
        }
        OrderedResolver { externs }
    }
}
